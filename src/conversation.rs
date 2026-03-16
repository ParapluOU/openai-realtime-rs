use crate::event::{Event, EventSender};
use crate::item::{BaseItem, BaseItemType, Item, ItemContentDelta, ItemStatus, Role};
use crate::types::*;
use crate::utils::RealtimeUtils;
use anyhow::{anyhow, Result};
use base64;
use log::{debug, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type ConversationHandlerReturntype = (Option<Item>, Option<ItemContentDelta>);

pub type ItemId = String;

/// RealtimeConversation holds conversation history
/// and performs event validation for RealtimeAPI
#[derive(Clone)]
pub struct RealtimeConversation {
    /// Default frequency for audio processing (24,000 Hz)
    pub default_frequency: usize,
    // todo: deduplicate the item_lookup and items
    /// Lookup table for items by their ID
    item_lookup: Arc<Mutex<HashMap<String, Item>>>,
    /// List of all items in the conversation
    items: Arc<Mutex<Vec<ItemId>>>,
    /// Lookup table for responses by their ID
    response_lookup: Arc<Mutex<HashMap<String, ResponseResourceType>>>,
    /// List of all responses in the conversation
    responses: Arc<Mutex<Vec<ResponseResourceType>>>,
    /// Queued speech items for processing
    queued_speech_items: Arc<Mutex<HashMap<String, QueuedSpeech>>>,
    /// Queued transcript items for processing
    queued_transcript_items: Arc<Mutex<HashMap<String, QueuedTranscript>>>,
    /// Queued input audio for processing
    queued_input_audio: Arc<Mutex<Option<Vec<i16>>>>,
    /// Event sender for sending conversation updates
    event_sender: EventSender,
}

impl RealtimeConversation {
    /// Create a new RealtimeConversation instance
    pub fn new(event_sender: EventSender) -> Self {
        Self {
            default_frequency: 24000,
            item_lookup: Arc::new(Mutex::new(HashMap::new())),
            items: Arc::new(Mutex::new(Vec::new())),
            response_lookup: Arc::new(Mutex::new(HashMap::new())),
            responses: Arc::new(Mutex::new(Vec::new())),
            queued_speech_items: Arc::new(Mutex::new(HashMap::new())),
            queued_transcript_items: Arc::new(Mutex::new(HashMap::new())),
            queued_input_audio: Arc::new(Mutex::new(None)),
            event_sender,
        }
    }

    /// Clears the conversation history and resets to default
    pub async fn clear(&mut self) {
        let mut item_lookup = self.item_lookup.lock().await;
        let mut items = self.items.lock().await;
        let mut response_lookup = self.response_lookup.lock().await;
        let mut responses = self.responses.lock().await;
        let mut queued_speech_items = self.queued_speech_items.lock().await;
        let mut queued_transcript_items = self.queued_transcript_items.lock().await;

        *item_lookup = HashMap::new();
        *items = Vec::new();
        *response_lookup = HashMap::new();
        *responses = Vec::new();
        *queued_speech_items = HashMap::new();
        *queued_transcript_items = HashMap::new();

        *self.queued_input_audio.lock().await = None;
    }

    /// Queue input audio for manual speech event
    pub async fn queue_input_audio(&self, input_audio: Vec<i16>) -> Vec<i16> {
        let mut queued_audio = self.queued_input_audio.lock().await;
        *queued_audio = Some(input_audio.clone());
        input_audio
    }

    /// Process an event from the WebSocket server and compose items
    /// equivalent to the EventProcessor in Convesration.js
    pub async fn process_event(&mut self, event: Event) -> Result<ConversationHandlerReturntype> {
        match event {
            // conversation.item.created
            Event::ConversationItemCreated { item } => self.handle_item_created(item).await,

            // conversation.item.truncated
            Event::ConversationItemTruncated {
                item_id,
                audio_end_ms,
            } => self.handle_item_truncated(item_id, audio_end_ms).await,

            // conversation.item.deleted
            Event::ConversationItemDeleted { item_id } => self.handle_item_deleted(item_id).await,

            // conversation.item.input_audio_transcription.completed
            Event::ConversationItemInputAudioTranscriptionCompleted {
                item_id,
                content_index,
                transcript,
            } => {
                info!("transcript: \"{}\"", &transcript);

                self.handle_input_audio_transcription_completed(item_id, content_index, transcript)
                    .await
            }

            // input_audio_buffer.speech_started
            Event::InputAudioBufferSpeechStarted {
                item_id,
                audio_start_ms,
            } => self.handle_speech_started(item_id, audio_start_ms).await,

            // input_audio_buffer.speech_stopped
            Event::InputAudioBufferSpeechStopped {
                item_id,
                audio_end_ms,
            } => self.handle_speech_stopped(item_id, audio_end_ms).await,

            // response.created
            Event::ResponseCreated { response } => self.handle_response_created(response).await,

            // response.output_item.added
            Event::ResponseOutputItemAdded {
                response_id,
                output_index,
                item,
            } => self.handle_output_item_added(response_id, item).await,

            // response.output_item.done
            Event::ResponseOutputItemDone(res) => self.handle_output_item_done(res.item).await,

            // response.content_part.added
            Event::ResponseContentPartAdded { item_id, part } => {
                self.handle_content_part_added(item_id, part).await
            }

            // response.audio_transcript.delta
            Event::ResponseAudioTranscriptDelta {
                item_id,
                content_index,
                delta,
            } => {
                self.handle_audio_transcript_delta(item_id, content_index, delta)
                    .await
            }

            // response.audio.delta
            Event::ResponseAudioDelta(evt) => self.handle_audio_delta(evt).await,

            // response.text.delta
            Event::ResponseTextDelta {
                item_id,
                content_index,
                delta,
            } => self.handle_text_delta(item_id, content_index, delta).await,

            // response.function_call_arguments.delta
            Event::ResponseFunctionCallArgumentsDelta { item_id, delta } => {
                self.handle_function_call_arguments_delta(item_id, delta)
                    .await
            }

            //
            _ => Ok((None, None)),
        }
    }

    /// Handle 'conversation.item.created' event.
    ///
    /// original js spec:
    ///
    ///     const { item } = event;
    //             // deep copy values
    //             const newItem = JSON.parse(JSON.stringify(item));
    //             if (!this.itemLookup[newItem.id]) {
    //                 this.itemLookup[newItem.id] = newItem;
    //                 this.items.push(newItem);
    //             }

    //             newItem.formatted = {};
    //             newItem.formatted.audio = new Int16Array(0);
    //             newItem.formatted.text = '';
    //             newItem.formatted.transcript = '';

    //             // If we have a speech item, can populate audio
    //             if (this.queuedSpeechItems[newItem.id]) {
    //                 newItem.formatted.audio = this.queuedSpeechItems[newItem.id].audio;
    //                 delete this.queuedSpeechItems[newItem.id]; // free up some memory
    //             }

    //             // Populate formatted text if it comes out on creation
    //             if (newItem.content) {
    //                 const textContent = newItem.content.filter((c) =>
    //                     ['text', 'input_text'].includes(c.type),
    //                 );
    //                 for (const content of textContent) {
    //                     newItem.formatted.text += content.text;
    //                 }
    //             }

    //             // If we have a transcript item, can pre-populate transcript
    //             if (this.queuedTranscriptItems[newItem.id]) {
    //                 newItem.formatted.transcript = this.queuedTranscriptItems.transcript;
    //                 delete this.queuedTranscriptItems[newItem.id];
    //             }

    //             if (newItem.type === 'message') {
    //                 if (newItem.role === 'user') {
    //                     newItem.status = 'completed';
    //                     if (this.queuedInputAudio) {
    //                         newItem.formatted.audio = this.queuedInputAudio;
    //                         this.queuedInputAudio = null;
    //                     }
    //                 } else {
    //                     newItem.status = 'in_progress';
    //                 }
    //             } else if (newItem.type === 'function_call') {
    //                 newItem.formatted.tool = {
    //                     type: 'function',
    //                     name: newItem.name,
    //                     call_id: newItem.call_id,
    //                     arguments: '',
    //                 };
    //                 newItem.status = 'in_progress';
    //             } else if (newItem.type === 'function_call_output') {
    //                 newItem.status = 'completed';
    //                 newItem.formatted.output = newItem.output;
    //             }
    //             return { item: newItem, delta: null };
    async fn handle_item_created(
        &mut self,
        mut item: Item,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        let mut items = self.items.lock().await;

        if !item_lookup.contains_key(&item.formatted.id) {
            item_lookup.insert(item.formatted.id.clone(), item.clone());
            items.push(item.formatted.id.clone());
        }

        // If we have a speech item, can populate audio
        if let Some(queued_speech) = self
            .queued_speech_items
            .lock()
            .await
            .remove(&item.formatted.id)
        {
            item.set_formatted_audio(queued_speech.audio.unwrap_or_default());
        }

        // If we have a transcript item, can pre-populate transcript
        if let Some(queued_transcript) = self
            .queued_transcript_items
            .lock()
            .await
            .remove(&item.formatted.id)
        {
            info!("transcript: {}", &queued_transcript.transcript);
            item.set_formatted_transcript(queued_transcript.transcript);
        }

        match item.get_type() {
            BaseItemType::Message => {
                if item.get_role() == Some(Role::User) {
                    item.base.set_status(ItemStatus::Completed);
                    if let Some(queued_input_audio) = self.queued_input_audio.lock().await.take() {
                        item.set_formatted_audio(queued_input_audio);
                    }
                } else {
                    item.base.set_status(ItemStatus::InProgress);
                }
            }
            BaseItemType::FunctionCall => {
                if let BaseItem::FunctionCall(function_call) = &item.base {
                    item.set_formatted_tool(FormattedTool {
                        tool_type: "function".to_string(),
                        name: function_call.name.clone(),
                        call_id: function_call.call_id.clone(),
                        arguments: Some(function_call.arguments.clone()),
                    });
                }
                item.base.set_status(ItemStatus::InProgress);
            }
            BaseItemType::FunctionCallOutput => {
                if let BaseItem::FunctionCallOutput(output) = &item.base {
                    item.set_formatted_output(output.output.clone());
                }
                item.base.set_status(ItemStatus::Completed);
            }
            _ => {}
        }

        self.event_sender
            .send(Event::ConversationItemCreated { item: item.clone() })
            .map_err(|e| anyhow!("Failed to send ConversationUpdated event: {}", e))?;

        Ok((Some(item), None))
    }

    /// Handle 'conversation.item.truncated' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, audio_end_ms } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(`item.truncated: Item "${item_id}" not found`);
    //             }
    //             const endIndex = Math.floor(
    //                 (audio_end_ms * this.defaultFrequency) / 1000,
    //             );
    //             item.formatted.transcript = '';
    //             item.formatted.audio = item.formatted.audio.slice(0, endIndex);
    //             return { item, delta: null };
    async fn handle_item_truncated(
        &self,
        item_id: String,
        audio_end_ms: u64,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        let item = item_lookup
            .get_mut(&item_id)
            .ok_or_else(|| anyhow!("Item '{}' not found", item_id))?;

        let end_index =
            ((audio_end_ms as f64 * self.default_frequency as f64) / 1000.0).floor() as usize;

        item.formatted.formatted.transcript = "".to_string();

        // todo: make helper on struct itself
        item.formatted.formatted.audio = item
            .formatted
            .formatted
            .audio
            .iter()
            .take(end_index)
            .cloned()
            .collect();

        Ok((Some(item.clone()), None))
    }

    /// Handle 'conversation.item.deleted' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(`item.deleted: Item "${item_id}" not found`);
    //             }
    //             delete this.itemLookup[item.id];
    //             const index = this.items.indexOf(item);
    //             if (index > -1) {
    //                 this.items.splice(index, 1);
    //             }
    //             return { item, delta: null };
    async fn handle_item_deleted(&self, item_id: String) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        let item = item_lookup
            .remove(&item_id)
            .ok_or_else(|| anyhow!("Item '{}' not found", item_id))?;

        let mut items = self.items.lock().await;
        items.retain(|i| i != &item.formatted.id);

        Ok((Some(item), None))
    }

    /// Handle 'conversation.item.input_audio_transcription.completed' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, content_index, transcript } = event;
    //             const item = this.itemLookup[item_id];
    //             // We use a single space to represent an empty transcript for .formatted values
    //             // Otherwise it looks like no transcript provided
    //             const formattedTranscript = transcript || ' ';
    //             if (!item) {
    //                 // We can receive transcripts in VAD mode before item.created
    //                 // This happens specifically when audio is empty
    //                 this.queuedTranscriptItems[item_id] = {
    //                     transcript: formattedTranscript,
    //                 };
    //                 return { item: null, delta: null };
    //             } else {
    //                 item.content[content_index].transcript = transcript;
    //                 item.formatted.transcript = formattedTranscript;
    //                 return { item, delta: { transcript } };
    //             }
    async fn handle_input_audio_transcription_completed(
        &self,
        item_id: String,
        content_index: usize,
        transcript: String,
    ) -> Result<ConversationHandlerReturntype> {
        let formatted_transcript = if transcript.is_empty() {
            " ".to_string()
        } else {
            transcript.clone()
        };

        let mut item_lookup = self.item_lookup.lock().await;

        // get item that was transcribed
        if let Some(item) = item_lookup.get_mut(&item_id) {
            item.set_content_transcript(content_index, &transcript);
            item.set_formatted_transcript(formatted_transcript);

            Ok((
                Some(item.clone()),
                Some(ItemContentDelta {
                    transcript: Some(transcript),
                    ..Default::default()
                }),
            ))
        }
        // if it doesnt exist
        else {
            let mut queued_transcript_items = self.queued_transcript_items.lock().await;
            queued_transcript_items.insert(
                item_id,
                QueuedTranscript {
                    transcript: formatted_transcript,
                },
            );
            Ok((None, None))
        }
    }

    /// Handle 'input_audio_buffer.speech_started' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, audio_start_ms } = event;
    //             this.queuedSpeechItems[item_id] = { audio_start_ms };
    //             return { item: null, delta: null };
    async fn handle_speech_started(
        &self,
        item_id: String,
        audio_start_ms: u64,
    ) -> Result<ConversationHandlerReturntype> {
        let mut queued_speech_items = self.queued_speech_items.lock().await;
        queued_speech_items.insert(
            item_id,
            QueuedSpeech {
                audio_start_ms,
                audio_end_ms: None,
                audio: None,
            },
        );

        Ok((None, None))
    }

    /// Handle 'input_audio_buffer.speech_stopped' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, audio_end_ms } = event;
    //             const speech = this.queuedSpeechItems[item_id];
    //             speech.audio_end_ms = audio_end_ms;
    //             if (inputAudioBuffer) {
    //                 const startIndex = Math.floor(
    //                     (speech.audio_start_ms * this.defaultFrequency) / 1000,
    //                 );
    //                 const endIndex = Math.floor(
    //                     (speech.audio_end_ms * this.defaultFrequency) / 1000,
    //                 );
    //                 speech.audio = inputAudioBuffer.slice(startIndex, endIndex);
    //             }
    //             return { item: null, delta: null };
    async fn handle_speech_stopped(
        &self,
        item_id: String,
        audio_end_ms: u64,
    ) -> Result<ConversationHandlerReturntype> {
        let mut queued_speech_items = self.queued_speech_items.lock().await;
        if let Some(speech) = queued_speech_items.get_mut(&item_id) {
            speech.audio_end_ms = Some(audio_end_ms);

            let queued_input_audio = self.queued_input_audio.lock().await;
            if let Some(input_audio) = &*queued_input_audio {
                let start_index = (speech.audio_start_ms * self.default_frequency as u64) / 1000;
                let end_index = (audio_end_ms * self.default_frequency as u64) / 1000;
                speech.audio = Some(input_audio[start_index as usize..end_index as usize].to_vec());
            }
        }

        Ok((None, None))
    }

    /// Handle 'response.created' event
    ///
    /// original JS spec:
    ///
    ///     const { response } = event;
    //             if (!this.responseLookup[response.id]) {
    //                 this.responseLookup[response.id] = response;
    //                 this.responses.push(response);
    //             }
    //             return { item: null, delta: null };
    async fn handle_response_created(
        &self,
        response: ResponseResourceType,
    ) -> Result<ConversationHandlerReturntype> {
        let response_id = response.id.clone();

        let mut response_lookup = self.response_lookup.lock().await;
        let mut responses = self.responses.lock().await;

        if !response_lookup.contains_key(&response_id) {
            response_lookup.insert(response_id.clone(), response.clone());
            responses.push(response);
            info!("registered new response");
        }

        Ok((None, None))
    }

    /// Handle 'response.output_item.added' event
    ///
    /// original JS spec:
    ///
    ///     const { response_id, item } = event;
    //             const response = this.responseLookup[response_id];
    //             if (!response) {
    //                 throw new Error(
    //                     `response.output_item.added: Response "${response_id}" not found`,
    //                 );
    //             }
    //             response.output.push(item.id);
    //             return { item: null, delta: null };
    async fn handle_output_item_added(
        &self,
        response_id: String,
        item: Item,
    ) -> Result<ConversationHandlerReturntype> {
        debug!("response output item added");

        let item_id = item.formatted.id.clone();

        let mut response_lookup = self.response_lookup.lock().await;
        if let Some(response) = response_lookup.get_mut(&response_id) {
            response.output.push(item);
            Ok((None, None))
        } else {
            Err(anyhow!("Response '{}' not found", response_id))
        }
    }

    /// Handle 'response.output_item.done' event
    ///
    /// original JS spec:
    ///
    ///     const { item } = event;
    //             if (!item) {
    //                 throw new Error(`response.output_item.done: Missing "item"`);
    //             }
    //             const foundItem = this.itemLookup[item.id];
    //             if (!foundItem) {
    //                 throw new Error(
    //                     `response.output_item.done: Item "${item.id}" not found`,
    //                 );
    //             }
    //             foundItem.status = item.status;
    //             return { item: foundItem, delta: null };
    async fn handle_output_item_done(&self, item: Item) -> Result<ConversationHandlerReturntype> {
        debug!("response output item done!");

        let item_id = item.formatted.id.clone();

        let mut item_lookup = self.item_lookup.lock().await;
        if let Some(found_item) = item_lookup.get_mut(&item_id) {
            found_item.base.set_status(item.base.status());
            Ok((Some(found_item.clone()), None))
        } else {
            Err(anyhow!("Item '{}' not found", item_id))
        }
    }

    /// Handle 'response.content_part.added' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, part } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(
    //                     `response.content_part.added: Item "${item_id}" not found`,
    //                 );
    //             }
    //             item.content.push(part);
    //             return { item, delta: null };
    async fn handle_content_part_added(
        &self,
        item_id: String,
        part: ContentPart,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;

        if let Some(item) = item_lookup.get_mut(&item_id) {
            match &mut item.base {
                BaseItem::Assistant(assistant_item) => {
                    assistant_item.content.push(match part {
                        ContentPart::Text(text_content) => AssistantContentType::Text(text_content),
                        ContentPart::Audio(audio_content) => {
                            AssistantContentType::Audio(audio_content)
                        }
                    });
                }
                BaseItem::User(user_item) => {
                    user_item.content.push(match part {
                        ContentPart::Text(text_content) => ContentPart::Text(TextContent {
                            content_type: text_content.content_type,
                            text: text_content.text,
                        }),
                        ContentPart::Audio(audio_content) => ContentPart::Audio(AudioContent {
                            content_type: audio_content.content_type,
                            audio: audio_content.audio,
                            transcript: audio_content.transcript,
                        }),
                    });
                }
                _ => {
                    return Err(anyhow!(
                        "Unexpected item type for content part addition: {:#?}",
                        &item.base
                    ))
                }
            }
            Ok((Some(item.clone()), None))
        } else {
            Err(anyhow!("Item '{}' not found", item_id))
        }
    }

    /// Handle 'response.audio_transcript.delta' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, content_index, delta } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(
    //                     `response.audio_transcript.delta: Item "${item_id}" not found`,
    //                 );
    //             }
    //             item.content[content_index].transcript += delta;
    //             item.formatted.transcript += delta;
    //             return { item, delta: { transcript: delta } };
    async fn handle_audio_transcript_delta(
        &self,
        item_id: String,
        content_index: usize,
        delta: String,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        if let Some(item) = item_lookup.get_mut(&item_id) {
            item.append_transcript(content_index, &delta);

            Ok((
                Some(item.clone()),
                Some(ItemContentDelta {
                    transcript: Some(delta),
                    ..Default::default()
                }),
            ))
        } else {
            Err(anyhow!("Item '{}' not found", item_id))
        }
    }

    /// Handle 'response.audio.delta' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, content_index, delta } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(`response.audio.delta: Item "${item_id}" not found`);
    //             }
    //             // This never gets renderered, we care about the file data instead
    //             // item.content[content_index].audio += delta;
    //             const arrayBuffer = RealtimeUtils.base64ToArrayBuffer(delta);
    //             const appendValues = new Int16Array(arrayBuffer);
    //             item.formatted.audio = RealtimeUtils.mergeInt16Arrays(
    //                 item.formatted.audio,
    //                 appendValues,
    //             );
    //             return { item, delta: { audio: appendValues } };
    async fn handle_audio_delta(
        &self,
        evt: ResponseAudioDeltaEvent,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        if let Some(item) = item_lookup.get_mut(&evt.item_id) {
            let array_buffer = RealtimeUtils::base64_to_array_buffer(&evt.delta);

            let append_values: Vec<i16> = array_buffer
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();

            item.append_formatted_audio(append_values.clone());

            Ok((
                Some(item.clone()),
                Some(ItemContentDelta {
                    audio: Some(append_values),
                    ..Default::default()
                }),
            ))
        } else {
            Err(anyhow!("Item '{}' not found", evt.item_id))
        }
    }

    /// Handle 'response.text.delta' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, content_index, delta } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(`response.text.delta: Item "${item_id}" not found`);
    //             }
    //             item.content[content_index].text += delta;
    //             item.formatted.text += delta;
    //             return { item, delta: { text: delta } };
    async fn handle_text_delta(
        &self,
        item_id: String,
        content_index: usize,
        delta: String,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        if let Some(item) = item_lookup.get_mut(&item_id) {
            item.append_transcript(content_index, &delta);
            Ok((
                Some(item.clone()),
                Some(ItemContentDelta {
                    text: Some(delta),
                    ..Default::default()
                }),
            ))
        } else {
            Err(anyhow!("Item '{}' not found", item_id))
        }
    }

    /// Handle 'response.function_call_arguments.delta' event
    ///
    /// original JS spec:
    ///
    ///     const { item_id, delta } = event;
    //             const item = this.itemLookup[item_id];
    //             if (!item) {
    //                 throw new Error(
    //                     `response.function_call_arguments.delta: Item "${item_id}" not found`,
    //                 );
    //             }
    //             item.arguments += delta;
    //             item.formatted.tool.arguments += delta;
    //             return { item, delta: { arguments: delta } };
    async fn handle_function_call_arguments_delta(
        &self,
        item_id: String,
        delta: String,
    ) -> Result<ConversationHandlerReturntype> {
        let mut item_lookup = self.item_lookup.lock().await;
        if let Some(item) = item_lookup.get_mut(&item_id) {
            if let BaseItem::FunctionCall(function_call) = &mut item.base {
                function_call.arguments += &delta;
            }

            if let Some(tool) = &mut item.formatted.formatted.tool {
                if let Some(tool_arguments) = &tool.arguments {
                    tool.arguments = Some(tool_arguments.to_string() + &delta);
                } else {
                    tool.arguments = Some(delta.clone());
                }
            }

            Ok((
                Some(item.clone()),
                Some(ItemContentDelta {
                    arguments: Some(delta),
                    ..Default::default()
                }),
            ))
        } else {
            Err(anyhow!("Item '{}' not found", item_id))
        }
    }

    /// Retrieves an item by id
    pub async fn get_item(&self, id: &str) -> Option<Item> {
        let item_lookup = self.item_lookup.lock().await;
        item_lookup.get(id).cloned()
    }

    pub async fn get_item_at(&self, idx: usize) -> Option<Item> {
        let map = self.items.lock().await;
        let id = map.get(idx)?;
        self.get_item(id).await
    }

    pub async fn map_item_at<R>(&self, idx: usize, mapper: impl Fn(&Item) -> R) -> Option<R> {
        let map = self.items.lock().await;
        let id = map.get(idx)?;
        self.get_item(id).await.as_ref().map(mapper)
    }

    /// Retrieves all items in the conversation
    pub async fn get_items(&self) -> Vec<Item> {
        let items = self.items.lock().await;

        let lookup = self.item_lookup.lock().await;

        items.iter().map(|id| lookup[id].clone()).collect()
    }
}
