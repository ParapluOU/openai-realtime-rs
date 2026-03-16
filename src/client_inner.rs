use crate::api::{RealtimeAPI, RealtimeAPISettings};
use crate::conversation::RealtimeConversation;
use crate::event::{Event, EventReceiver, EventSender};
use crate::item::{BaseItem, BaseItemType, FormattedItem, Item, ItemStatus, Role, UserItem};
use crate::session::{
    SessionConfig, SessionConfigUpdate, SessionResourceContainer, ToolDefinition, ToolType, VADType,
};
use crate::tool_handler::ToolHandler;
use crate::types::{ApiEvent, ContentPart, ConversationItemTruncate, ResponseResourceType};
use crate::utils::RealtimeUtils;
use anyhow::{anyhow, Context};
use async_recursion::async_recursion;
use log::{debug, info, trace, warn};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;

/// The main client for interacting with the Realtime API.
pub struct RealtimeClientInner {
    /// original config as it was passed in, before it might have been changed along the way
    default_session_config: SessionConfig,

    /// active current config
    session_config: SessionConfig,

    /// inner realtime api driver
    realtime: RealtimeAPI,

    /// conversation state
    conversation: RealtimeConversation,

    /// whether teh session was initialized
    session_created: bool,

    /// active tools
    tools: HashMap<String, ToolHandler>,

    /// buffer for user audio
    input_audio_buffer: Vec<i16>,

    /// senders for async communication
    event_sender: broadcast::Sender<Event>,
}

impl Deref for RealtimeClientInner {
    type Target = RealtimeAPI;

    fn deref(&self) -> &Self::Target {
        &self.realtime
    }
}

impl DerefMut for RealtimeClientInner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.realtime
    }
}

impl RealtimeClientInner {
    pub fn new(key: &str, config: SessionConfig, sender: EventSender) -> Self {
        debug!("initialized new RealtimeClient with config: {:#?}", config);

        Self {
            default_session_config: config.clone(),
            session_config: config,
            realtime: RealtimeAPI::new(
                RealtimeAPISettings {
                    api_key: key.to_string().into(),
                    debug: true,
                },
                sender.clone(),
            ),
            conversation: RealtimeConversation::new(sender.clone()),
            session_created: false,
            tools: Default::default(),
            input_audio_buffer: vec![],
            event_sender: sender,
        }
    }

    fn reset_config(&mut self) -> &mut Self {
        self.session_created = false;
        self.tools.clear();
        self.session_config = self.default_session_config.clone();
        self.input_audio_buffer.clear();

        self
    }

    pub async fn connect(&mut self) -> anyhow::Result<()> {
        if self.realtime.is_connected() {
            return Err(anyhow!("Already connected, use .disconnect() first"));
        }

        // todo: handle error differently?
        if let Err(e) = self.realtime.connect(None).await {
            panic!("{}", e);
        };

        // // todo: handle error differently?
        // if let Err(e) = self.update_session_default().await {
        //     panic!("{}", e);
        // }

        Ok(())
    }

    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.session_created = false;

        self.conversation.clear().await;

        if self.realtime.is_connected() {
            self.realtime.disconnect().await;
        }

        Ok(())
    }

    pub fn get_turn_detection_type(&self) -> Option<VADType> {
        self.session_config
            .turn_detection
            .as_ref()
            .map(|td| td.vad_type)
    }

    pub async fn add_tool_handler(
        &mut self,
        tool_name: &str,
        handler: Box<dyn Fn(Value) -> anyhow::Result<Value> + Send>,
    ) -> anyhow::Result<&ToolHandler> {
        let tool_def = self
            .session_config
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or(anyhow!("tool not found for {}", tool_name))?;

        self.add_tool(tool_def.clone(), handler).await
    }

    pub async fn add_tool(
        &mut self,
        definition: ToolDefinition,
        handler: Box<dyn Fn(Value) -> anyhow::Result<Value> + Send>,
    ) -> anyhow::Result<&ToolHandler> {
        let name = definition.name.clone();

        if self.tools.contains_key(&name) {
            return Err(anyhow!("Tool \"{}\" already added", &name));
        }

        self.tools.insert(
            name.clone(),
            ToolHandler {
                definition,
                handler: Arc::new(tokio::sync::Mutex::new(handler)),
            },
        );

        self.update_session_default().await?;

        Ok(self.tools.get(&name).unwrap())
    }

    pub async fn remove_tool(&mut self, name: &str) -> anyhow::Result<()> {
        if !self.tools.contains_key(name) {
            return Err(anyhow!(
                "Tool \"{}\" does not exist, cannot be removed.",
                name
            ));
        }
        self.tools.remove(name);
        Ok(())
    }

    pub async fn delete_item(&mut self, id: &str) -> anyhow::Result<()> {
        self.realtime
            .send("conversation.item.delete", json!({ "item_id": id }).into())
            .await
            .map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }

    pub async fn update_session_default(&mut self) -> anyhow::Result<()> {
        self.update_session(SessionConfigUpdate::default()).await
    }

    pub async fn update_session(
        &mut self,
        session_config: SessionConfigUpdate,
    ) -> anyhow::Result<()> {
        // Update session config fields
        if let Some(ref modalities) = session_config.modalities {
            self.session_config.modalities = modalities.clone();
        }
        if let Some(ref instructions) = &session_config.instructions {
            self.session_config.instructions = instructions.clone();
        }
        if let Some(ref voice) = session_config.voice {
            self.session_config.voice = *voice;
        }
        if let Some(ref input_audio_format) = session_config.input_audio_format {
            self.session_config.input_audio_format = *input_audio_format;
        }
        if let Some(ref output_audio_format) = session_config.output_audio_format {
            self.session_config.output_audio_format = *output_audio_format;
        }
        if let Some(ref input_audio_transcription) = session_config.input_audio_transcription {
            self.session_config.input_audio_transcription = Some(input_audio_transcription.clone());
        }
        if let Some(ref turn_detection) = session_config.turn_detection {
            self.session_config.turn_detection = turn_detection.clone();
        }
        if let Some(ref tool_choice) = session_config.tool_choice {
            self.session_config.tool_choice = (tool_choice.clone());
        }
        if let Some(ref temperature) = session_config.temperature {
            self.session_config.temperature = (temperature.clone());
        }
        if let Some(ref max_response_output_tokens) = session_config.max_response_output_tokens {
            self.session_config.max_response_output_tokens = (max_response_output_tokens.clone());
        }

        let tool_changes_made = self.update_session_tools(&session_config)?;

        // reformat the instruction string that might have been defined in an editor
        self.session_config.instructions = self
            .session_config
            .instructions
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ");

        assert!(!self.session_config.tools.is_empty());

        // Send updated session configuration to the server
        if self.realtime.is_connected() {
            self.realtime
                .send(
                    "session.update",
                    Some(SessionResourceContainer::from(self.session_config.clone())),
                )
                .await?;
        }

        Ok(())
    }

    //
    // merge remote and local tools
    //
    pub fn update_session_tools(
        &mut self,
        session_config: &SessionConfigUpdate,
    ) -> anyhow::Result<bool> {
        let mut changed = false;

        // Load tools from tool definitions + already loaded tools
        let mut use_tools = self.session_config.tools.clone();

        if let Some(tools) = &session_config.tools {
            for tool_definition in tools {
                let definition = ToolDefinition {
                    tool_type: ToolType::Function,
                    ..tool_definition.clone()
                };

                if self.tools.contains_key(&definition.name) {
                    return Err(anyhow!(
                        "Tool \"{}\" has already been defined",
                        definition.name
                    ));
                }

                use_tools.push(definition);

                changed = true;
            }
        }

        use_tools.extend(self.tools.values().map(|tool| tool.definition.clone()));

        self.session_config.tools = use_tools;

        if changed {
            info!(
                "defined function calling tools: {:#?}",
                &self.session_config.tools
            );
        }

        Ok(changed)
    }

    pub async fn send_user_message_content(
        &mut self,
        content: Vec<impl Into<ContentPart>>,
    ) -> anyhow::Result<()> {
        if !content.is_empty() {
            let content = content.into_iter().map(Into::into).collect();

            let item = Item {
                base: BaseItem::User(UserItem {
                    // todo: take last conversation item id
                    // previous_item_id: None,
                    item_type: BaseItemType::Message,
                    status: ItemStatus::Completed,
                    role: Role::User,
                    content,
                }),
                formatted: FormattedItem {
                    id: RealtimeUtils::generate_id("item_", 16),
                    object: "conversation.item".to_string(),
                    previous_item_id: None,
                    formatted: Default::default(),
                },
            };

            self.realtime
                .send(
                    "conversation.item.create",
                    Some(json!({"item": serde_json::to_value(item).unwrap()})),
                )
                .await?;
        }

        self.create_response().await?;

        Ok(())
    }

    pub async fn append_input_audio(&mut self, array_buffer: Vec<i16>) -> anyhow::Result<()> {
        assert!(!array_buffer.is_empty());

        self.realtime
            .send(
                "input_audio_buffer.append",
                json!({
                    "audio": RealtimeUtils::array_buffer_to_base64(&array_buffer),
                })
                .into(),
            )
            .await?;

        self.input_audio_buffer.extend(array_buffer);

        Ok(())
    }

    // todo: check if this works
    pub async fn append_input_audio_file(&mut self, path: &Path) -> anyhow::Result<()> {
        debug!("appending audio file {} to input buffer...", path.display());

        let audio_samples = RealtimeUtils::samples_from_file(path)?;

        self.append_input_audio(audio_samples).await?;

        // self.commit_and_queue().await

        Ok(())
    }

    pub async fn commit(&mut self) -> anyhow::Result<()> {
        self.realtime
            .send("input_audio_buffer.commit", json!({}).into())
            .await
    }

    pub async fn queue_input_audio(&mut self) -> anyhow::Result<Vec<i16>> {
        let res = self
            .conversation
            .queue_input_audio(self.input_audio_buffer.clone())
            .await;

        self.input_audio_buffer.clear();

        Ok(res)
    }

    pub async fn commit_and_queue(&mut self) -> anyhow::Result<()> {
        self.commit().await?;
        self.queue_input_audio().await?;
        Ok(())
    }

    pub async fn create_response(&mut self) -> anyhow::Result<()> {
        if self.get_turn_detection_type().is_none() && !self.input_audio_buffer.is_empty() {
            self.commit_and_queue().await?;
        }

        self.realtime
            .send("response.create", json!({}).into())
            .await?;

        Ok(())
    }

    pub async fn cancel_response(
        &mut self,
        id: &str,
        sample_count: usize,
    ) -> anyhow::Result<Option<Item>> {
        if id.is_empty() {
            self.realtime.send::<()>("response.cancel", None).await?;
            return Ok(None);
        }

        let item = self
            .conversation
            .get_item(id)
            .await
            .ok_or_else(|| anyhow!("Could not find item \"{}\"", id))?;

        if let BaseItem::Assistant(assistant_item) = &item.base {
            if assistant_item.item_type != BaseItemType::Message {
                return Err(anyhow!("Can only cancel messages with type 'message'."));
            }
        } else {
            return Err(anyhow!("Can only cancel messages with role 'assistant'."));
        }

        self.realtime.send::<()>("response.cancel", None).await?;

        let audio_end_ms = ((sample_count as f64 / self.conversation.default_frequency as f64)
            * 1000.0)
            .floor() as i64;

        self.realtime
            .send(
                "conversation.item.truncate",
                Some(ConversationItemTruncate {
                    item_id: id.to_string(),
                    audio_end_ms,
                }),
            )
            .await?;

        Ok(Some(item))
    }

    pub async fn call_tool(
        &mut self,
        call_id: &str,
        tool_name: &str,
        json_arguments: Value,
    ) -> anyhow::Result<()> {
        info!("calling function {}({:?})", tool_name, &json_arguments);

        let tool_config = self
            .tools
            .get(tool_name)
            .cloned()
            .ok_or_else(|| anyhow!("Tool \"{}\" has not been added", tool_name))?;

        let handler = tool_config.handler.lock().await;

        match (handler)(json_arguments) {
            Ok(result) => {
                self.realtime
                    .send(
                        "conversation.item.create",
                        Some(json!({
                            "item": {
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": serde_json::to_string(&result)?,
                            }
                        })),
                    )
                    .await
                    .context("Failed to send function call output")?;

                self.create_response().await?;

                Ok(())
            }
            Err(e) => {
                self.realtime
                    .send(
                        "conversation.item.create",
                        Some(json!({
                            "item": {
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": json!({"error": e.to_string()}),
                            }
                        })),
                    )
                    .await
                    .context("Failed to send function call error")?;

                self.create_response().await?;

                Err(e)
            }
        }
    }

    /// handle an incoming server event by parsing it into the right subevent for the Conversation.
    /// the conversation will then adjust its inner state
    #[async_recursion]
    async fn handle_server_event(&mut self, mut event: Event) -> anyhow::Result<()> {
        if let Event::ServerEvent(event_data) = event {
            // reinterpret event
            // debug!("reinterpreting Server event... {:#?}", &event_data);
            let err = format!("failed to serialize {:#?}", &event_data);
            let json = serde_json::to_value(event_data).expect(&err);

            event = serde_json::from_value(json.clone()).expect(&format!(
                "failed to deserialize json into Event: {:#?}",
                json
            ));

            trace!("reinterpreted server Event into {:#?}", &event);

            // update converstion state. will do nothing if it is an event that is more meta
            self.conversation.process_event(event.clone()).await?;

            // handle control events
            self.handle_event(event).await?;
        } else {
            panic!("unsupported event type: {:#?}", event);
        }

        Ok(())
    }

    async fn handle_client_event(
        &self,
        event_type: &str,
        event_data: ApiEvent<Value>,
    ) -> anyhow::Result<()> {
        // Process client events if needed
        Ok(())
    }

    #[async_recursion]
    pub async fn handle_event(&mut self, event: Event) -> anyhow::Result<bool> {
        // debug!("EventLoop processing event: {:#?}", &event);
        // debug!(
        //     "EventLoop processing event: {}",
        //     serde_json::to_string_pretty(&event).unwrap()
        // );

        match event {
            Event::ServerEvent(event_data) => {
                self.handle_server_event(Event::ServerEvent(event_data))
                    .await?;
            }
            Event::ClientEvent {
                event_type,
                event_data,
            } => {
                self.handle_client_event(&event_type, event_data).await?;
            }
            // Event::ConversationItemCreated { item } => {
            //     self.handle_conversation_item_created(item).await?;
            // }
            // Event::ConversationItemTruncated {
            //     item_id,
            //     audio_end_ms,
            // } => {
            //     self.handle_conversation_item_truncated(item_id, audio_end_ms)
            //         .await?;
            // }
            // Event::ConversationItemDeleted { item_id } => {
            //     self.handle_conversation_item_deleted(item_id).await?;
            // }
            // Event::ConversationItemInputAudioTranscriptionCompleted {
            //     item_id,
            //     content_index,
            //     transcript,
            // } => {
            //     self.handle_conversation_item_input_audio_transcription_completed(
            //         item_id,
            //         content_index,
            //         transcript,
            //     )
            //     .await?;
            // }
            // Event::InputAudioBufferSpeechStarted {
            //     item_id,
            //     audio_start_ms,
            // } => {
            //     self.handle_input_audio_buffer_speech_started(item_id, audio_start_ms)
            //         .await?;
            // }
            // Event::InputAudioBufferSpeechStopped {
            //     item_id,
            //     audio_end_ms,
            // } => {
            //     self.handle_input_audio_buffer_speech_stopped(item_id, audio_end_ms)
            //         .await?;
            // }
            // Event::ResponseCreated { response } => {
            //     self.handle_response_created(response).await?;
            // }
            // Event::ResponseOutputItemAdded {
            //     response_id,
            //     output_index,
            //     item,
            // } => {
            //     self.handle_response_output_item_added(response_id, output_index, item)
            //         .await?;
            // }
            // Event::ResponseOutputItemDone(item) => {
            //     self.handle_response_output_item_done(item).await?;
            // }
            // Event::ResponseContentPartAdded { item_id, part } => {
            //     self.handle_response_content_part_added(item_id, part)
            //         .await?;
            // }
            // Event::ResponseAudioTranscriptDelta {
            //     item_id,
            //     content_index,
            //     delta,
            // } => {
            //     self.handle_response_audio_transcript_delta(item_id, content_index, delta)
            //         .await?;
            // }
            // Event::ResponseAudioDelta { item_id, delta } => {
            //     self.handle_response_audio_delta(item_id, delta).await?;
            // }
            // Event::ResponseTextDelta {
            //     item_id,
            //     content_index,
            //     delta,
            // } => {
            //     self.handle_response_text_delta(item_id, content_index, delta)
            //         .await?;
            // }
            // Event::ResponseFunctionCallArgumentsDelta { item_id, delta } => {
            //     self.handle_response_function_call_arguments_delta(item_id, delta)
            //         .await?;
            // }
            // Event::ConversationItemAppended { .. } => {
            //     // todo: shoujld anything be done here?
            // }
            Event::ResponseFunctionCallArgumentsDone {
                item_id,
                name,
                arguments,
                output_index,
                call_id,
            } => {
                self.call_tool(&call_id, &name, serde_json::from_str(&arguments)?)
                    .await?;
            }
            Event::Close { .. } => {
                warn!("websocket closed by server");
                return Ok(true);
            }
            Event::SessionCreated { .. } => {
                self.session_created = true;
            }
            // Event::SessionUpdated { .. } => {}
            // Event::ResponseDone { .. } => {}
            Event::Ping { data } => {
                let _ = self.pong(data).await;
            }
            _ => {}
        }

        Ok(false)
    }

    async fn handle_conversation_item_created(&self, item: Item) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ConversationItemCreated { item })
        //     .map_err(|e| anyhow!("Failed to send ConversationItemCreated event: {}", e))?;
        Ok(())
    }

    async fn handle_conversation_item_truncated(
        &self,
        item_id: String,
        audio_end_ms: u64,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ConversationItemTruncated {
        //         item_id,
        //         audio_end_ms,
        //     })
        //     .map_err(|e| anyhow!("Failed to send ConversationItemTruncated event: {}", e))?;
        Ok(())
    }

    async fn handle_conversation_item_deleted(&self, item_id: String) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ConversationItemDeleted { item_id })
        //     .map_err(|e| anyhow!("Failed to send ConversationItemDeleted event: {}", e))?;
        Ok(())
    }

    async fn handle_conversation_item_input_audio_transcription_completed(
        &self,
        item_id: String,
        content_index: usize,
        transcript: String,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ConversationItemInputAudioTranscriptionCompleted {
        //         item_id,
        //         content_index,
        //         transcript,
        //     })
        //     .map_err(|e| {
        //         anyhow!(
        //             "Failed to send ConversationItemInputAudioTranscriptionCompleted event: {}",
        //             e
        //         )
        //     })?;
        Ok(())
    }

    async fn handle_input_audio_buffer_speech_started(
        &self,
        item_id: String,
        audio_start_ms: u64,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::InputAudioBufferSpeechStarted {
        //         item_id,
        //         audio_start_ms,
        //     })
        //     .map_err(|e| anyhow!("Failed to send InputAudioBufferSpeechStarted event: {}", e))?;
        Ok(())
    }

    async fn handle_input_audio_buffer_speech_stopped(
        &self,
        item_id: String,
        audio_end_ms: u64,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::InputAudioBufferSpeechStopped {
        //         item_id,
        //         audio_end_ms,
        //     })
        //     .map_err(|e| anyhow!("Failed to send InputAudioBufferSpeechStopped event: {}", e))?;
        Ok(())
    }

    async fn handle_response_created(&self, response: ResponseResourceType) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseCreated { response })
        //     .map_err(|e| anyhow!("Failed to send ResponseCreated event: {}", e))?;
        Ok(())
    }

    async fn handle_response_output_item_added(
        &self,
        response_id: String,
        output_index: u32,
        item: Item,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseOutputItemAdded {
        //         response_id,
        //         output_index,
        //         item,
        //     })
        //     .map_err(|e| anyhow!("Failed to send ResponseOutputItemAdded event: {}", e))?;
        Ok(())
    }

    async fn handle_response_output_item_done(&self, item: Item) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseOutputItemDone(item))
        //     .map_err(|e| anyhow!("Failed to send ResponseOutputItemDone event: {}", e))?;
        Ok(())
    }

    async fn handle_response_content_part_added(
        &self,
        item_id: String,
        part: ContentPart,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseContentPartAdded { item_id, part })
        //     .map_err(|e| anyhow!("Failed to send ResponseContentPartAdded event: {}", e))?;
        Ok(())
    }

    async fn handle_response_audio_transcript_delta(
        &self,
        item_id: String,
        content_index: usize,
        delta: String,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseAudioTranscriptDelta {
        //         item_id,
        //         content_index,
        //         delta,
        //     })
        //     .map_err(|e| anyhow!("Failed to send ResponseAudioTranscriptDelta event: {}", e))?;
        Ok(())
    }

    async fn handle_response_audio_delta(
        &self,
        item_id: String,
        delta: String,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseAudioDelta { item_id, delta })
        //     .map_err(|e| anyhow!("Failed to send ResponseAudioDelta event: {}", e))?;
        Ok(())
    }

    async fn handle_response_text_delta(
        &self,
        item_id: String,
        content_index: usize,
        delta: String,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseTextDelta {
        //         item_id,
        //         content_index,
        //         delta,
        //     })
        //     .map_err(|e| anyhow!("Failed to send ResponseTextDelta event: {}", e))?;
        Ok(())
    }

    async fn handle_response_function_call_arguments_delta(
        &self,
        item_id: String,
        delta: String,
    ) -> anyhow::Result<()> {
        // self.event_sender
        //     .send(Event::ResponseFunctionCallArgumentsDelta { item_id, delta })
        //     .map_err(|e| {
        //         anyhow!(
        //             "Failed to send ResponseFunctionCallArgumentsDelta event: {}",
        //             e
        //         )
        //     })?;
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.realtime.is_connected()
    }

    pub fn is_session_created(&self) -> bool {
        self.session_created
    }

    pub fn set_session_created(&mut self) {
        self.session_created = true;
    }

    pub fn conversation(&self) -> &RealtimeConversation {
        &self.conversation
    }

    pub fn conversation_mut(&mut self) -> &mut RealtimeConversation {
        &mut self.conversation
    }

    pub fn on_event(&self) -> EventReceiver {
        self.event_sender.subscribe()
    }

    pub async fn reset(&mut self) -> anyhow::Result<()> {
        self.disconnect().await?;
        self.reset_config();
        Ok(())
    }
}
