use crate::client_inner::RealtimeClientInner;
use crate::event::Event;
use crate::item::Item;
use crate::iter::RealtimeAudioIter;
use crate::player::Player;
use crate::recorder::Recorder;
use crate::session::{SessionConfig, SessionConfigUpdate, ToolDefinition, VADType};
use crate::types::*;
use crate::utils::RealtimeUtils;
use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};
use rodio::{OutputStream, Source};
use serde_json::Value;
use std::any::type_name;
use std::env;
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use tap::{Pipe, Tap};
use tokio::sync::{broadcast, mpsc};
use tokio::task;
use tokio::task::JoinHandle;
use tokio::time::Duration;

pub type Client = RealtimeClient;

#[derive(Clone)]
pub struct RealtimeClient {
    inner: Arc<tokio::sync::Mutex<RealtimeClientInner>>,

    /// flag to indicate when the eventloop is ready
    event_loop_ready: Arc<tokio::sync::Mutex<bool>>,
}

impl RealtimeClient {
    pub async fn start(
        config: Option<SessionConfig>,
    ) -> anyhow::Result<(Self, task::JoinHandle<anyhow::Result<()>>)> {
        let instance = Self::new_with_conf(config.unwrap_or_default());

        // start event processor before we try initialize the session
        let event_loop_handle = instance.spawn_event_loop().await;

        instance.connect().await?;

        // perform first update-session call which should result in us receiving session.created
        debug!("updating session for the first time...");
        instance.update_session_default().await?;

        // wait for session confirmation before we add audio
        instance.wait_for_session_created().await?;

        Ok((instance.clone(), event_loop_handle))
    }

    /// iterator over all samples in the buffer
    pub fn audio_stream(&self) -> (RealtimeAudioIter, mpsc::Sender<Vec<i16>>) {
        // Create an mpsc channel to send audio data
        let (tx, rx) = mpsc::channel(100);

        (RealtimeAudioIter::new(self.clone(), rx), tx)
    }

    /// start playing audio stream
    #[must_use]
    pub fn start_play_audio_stream(&self) -> Player {
        Player::play(self.clone())
    }

    #[must_use]
    pub fn start_mic_input_listener(&self, vad_threshold: Option<f32>) -> anyhow::Result<Recorder> {
        Recorder::capture(self.clone(), vad_threshold)
    }

    pub fn new() -> Self {
        Self::new_with_api_key(
            &env::var("PARTURE_OPENAI_API_KEY")
                .expect("expected environment to have OPENAI_API_KEY"),
        )
    }

    pub fn new_with_conf(config: SessionConfig) -> Self {
        Self::new_with_key_and_conf(
            &env::var("PARTURE_OPENAI_API_KEY")
                .expect("expected environment to have OPENAI_API_KEY"),
            config,
        )
    }

    pub fn new_with_api_key(key: &str) -> Self {
        Self::new_with_key_and_conf(key, SessionConfig::default())
    }

    pub fn new_with_key_and_conf(key: &str, config: SessionConfig) -> Self {
        let (event_sender, event_receiver) = broadcast::channel(100);

        let inner = RealtimeClientInner::new(key, config.clone(), event_sender.clone());

        Self {
            inner: Arc::new(tokio::sync::Mutex::new(inner)),
            // event_receiver: Arc::new(tokio::sync::Mutex::new(event_receiver)),
            event_loop_ready: Arc::new(tokio::sync::Mutex::new(false)),
        }
    }

    pub async fn is_eventloop_ready(&self) -> bool {
        *self.event_loop_ready.lock().await
    }

    pub async fn connect(&self) -> Result<()> {
        self.inner.lock().await.connect().await
    }

    pub async fn wait_for_session_created(&self) -> Result<()> {
        if !self.is_connected().await {
            return Err(anyhow!("Not connected, use .connect() first"));
        }

        debug!("waiting for session to be created...");

        while !self.inner.lock().await.is_session_created() {
            task::yield_now().await;
        }

        Ok(())
    }

    // todo: originally the JS retrieves a sub-object called "item". we might still have to do that or create a wrapper for ItemType
    // with that kind of field
    pub async fn wait_for_next_item(&mut self, timeout: Option<u64>) -> Result<Item> {
        Ok(self
            .wait_for_next::<Item>("conversation.item.appended", timeout)
            .await?)
    }

    pub async fn wait_for_next_completed_item(&mut self, timeout: Option<u64>) -> Result<Item> {
        Ok(self
            .wait_for_next::<Item>("conversation.item.completed", timeout)
            .await?)
    }

    pub async fn wait_for_next<T: serde::de::DeserializeOwned>(
        &self,
        event_name: &str,
        timeout: Option<u64>,
    ) -> Result<T> {
        let mut listener = self.inner.lock().await.on_event();

        let fut = async {
            loop {
                if let Ok(event) = listener.recv().await {
                    debug!(
                        "observed Event {} while waiting for {}...",
                        event.get_type(),
                        type_name::<T>(),
                    );

                    match event {
                        Event::ServerEvent(event_data) if event_data.event_type == event_name => {
                            info!("successfully waited for event {}", &event_name);

                            let ctx = format!(
                                "deserializing event in wait_for_next(): {:#?}",
                                &event_data.data,
                            );

                            return Ok(serde_json::from_value(
                                event_data.data.unwrap_or(Value::Null),
                            )
                            .context(ctx)?);
                        }
                        _ => continue,
                    }
                }
            }
        };

        if let Some(t) = timeout {
            tokio::time::timeout(Duration::from_secs(t), fut)
                .await
                .map_err(|_| anyhow!("Timeout waiting for event: {}", event_name))?
        } else {
            fut.await
        }
    }

    pub async fn disconnect(&self) -> Result<()> {
        self.inner.lock().await.disconnect().await
    }

    pub async fn get_turn_detection_type(&self) -> Option<VADType> {
        self.inner.lock().await.get_turn_detection_type()
    }

    pub async fn get_items(&self) -> Vec<Item> {
        self.inner.lock().await.conversation().get_items().await
    }

    pub async fn get_item_at(&self, offset: usize) -> Option<Item> {
        self.inner
            .lock()
            .await
            .conversation()
            .get_item_at(offset)
            .await
    }

    pub async fn map_item_at<R>(&self, offset: usize, mapper: impl Fn(&Item) -> R) -> Option<R> {
        self.inner
            .lock()
            .await
            .conversation()
            .map_item_at(offset, mapper)
            .await
    }

    /// the difference of this with the tool definitions in the config
    /// is that the tool config defines the tools on the AI side, whereas this function is needed to register the handler
    /// that should be called when a tool is invoked
    pub async fn add_tool(
        &self,
        definition: ToolDefinition,
        handler: Box<dyn Fn(Value) -> Result<Value> + Send>,
    ) -> Result<()> {
        self.inner
            .lock()
            .await
            .add_tool(definition, handler)
            .await
            .unwrap();
        Ok(())
    }

    pub async fn add_tool_handler(
        &self,
        tool_name: &str,
        handler: Box<dyn Fn(Value) -> Result<Value> + Send>,
    ) -> Result<()> {
        self.inner
            .lock()
            .await
            .add_tool_handler(tool_name, handler)
            .await
            .unwrap();
        Ok(())
    }

    pub async fn remove_tool(&self, name: &str) -> Result<()> {
        self.inner.lock().await.remove_tool(name).await
    }

    pub async fn delete_item(&self, id: &str) -> Result<()> {
        self.inner.lock().await.delete_item(id).await
    }

    pub async fn update_session_default(&self) -> Result<()> {
        self.inner.lock().await.update_session_default().await
    }

    pub async fn update_session(&self, session_config: SessionConfigUpdate) -> Result<()> {
        self.inner.lock().await.update_session(session_config).await
    }

    pub async fn send_user_message_content(
        &self,
        content: Vec<impl Into<ContentPart>>,
    ) -> Result<()> {
        self.inner
            .lock()
            .await
            .send_user_message_content(content)
            .await
    }

    pub async fn append_input_audio(&self, array_buffer: Vec<i16>) -> Result<()> {
        if array_buffer.iter().all(|v| v == &0) {
            warn!("did not append chunk that contains only silence");
            return Ok(());
        }

        self.inner
            .lock()
            .await
            .append_input_audio(array_buffer)
            .await
    }

    pub async fn append_input_audio_file(&self, path: &Path) -> Result<()> {
        self.inner.lock().await.append_input_audio_file(path).await
    }

    pub async fn create_response(&self) -> Result<()> {
        self.inner.lock().await.create_response().await
    }

    pub async fn cancel_response(&self, id: &str, sample_count: usize) -> Result<Option<Item>> {
        self.inner
            .lock()
            .await
            .cancel_response(id, sample_count)
            .await
    }

    pub async fn call_tool(&self, tool_name: &str, json_arguments: Value) -> Result<()> {
        self.inner
            .lock()
            .await
            .call_tool(
                &RealtimeUtils::generate_id("call_", 16),
                tool_name,
                json_arguments,
            )
            .await
    }

    pub async fn is_connected(&self) -> bool {
        self.inner.lock().await.is_connected()
    }

    pub async fn reset(&self) -> anyhow::Result<()> {
        self.inner.lock().await.reset().await
    }

    pub async fn spawn_event_loop(&self) -> tokio::task::JoinHandle<Result<()>> {
        debug!("spawning event loop...");
        let cloned = self.clone();

        let handle = tokio::spawn(async move { cloned.run().await });

        // todo: doesnt work
        // while !self.is_eventloop_ready().await {
        //     task::yield_now().await;
        //     debug!("eventloop not ready");
        // }

        tokio::time::sleep(Duration::from_secs(1)).await;

        debug!("eventloop ready");

        handle
    }

    // custom function to run eventloop, because we use mpsc channels
    // instead of a global eventhandler
    pub async fn run(self) -> Result<()> {
        debug!("starting event loop...");

        let mut event_receiver = self.inner.lock().await.on_event();

        let mut flag_lock = self.event_loop_ready.lock().await;
        *flag_lock = true;

        debug!("marked eventloop as started");

        while let Ok(event) = event_receiver.recv().await {
            let should_stop = self.inner.lock().await.handle_event(event).await?;

            if should_stop {
                debug!("event loop stopping requested from RealtimeClientInner event handling");
                break;
            }
        }
        Ok(())
    }
}
