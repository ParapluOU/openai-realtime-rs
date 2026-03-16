use crate::event::{Event, EventSender};
use crate::types::*;
use crate::utils::RealtimeUtils;
use anyhow::anyhow;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use log::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;

// Add this struct for the rate limiter
struct RateLimiter {
    last_request: Instant,
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            last_request: Instant::now(),
            tokens: 1.0,
            max_tokens: 1.0,
            refill_rate: 1.0, // 1 token per second
        }
    }

    async fn acquire(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_request).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);

        if self.tokens < 1.0 {
            let wait_time = Duration::from_secs_f64((1.0 - self.tokens) / self.refill_rate);
            tokio::time::sleep(wait_time).await;
            self.tokens = 1.0;
        }

        self.tokens -= 1.0;
        self.last_request = Instant::now();
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct RealtimeAPISettings {
    pub api_key: Option<String>,
    pub debug: bool,
}

pub struct RealtimeAPI {
    api_key: Option<String>,
    url: String,
    ws: Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    debug: bool,
    event_sender: EventSender,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl RealtimeAPI {
    pub fn new(settings: RealtimeAPISettings, event_sender: EventSender) -> Self {
        let url = "wss://api.openai.com/v1/realtime".to_string();
        let api_key = settings.api_key.clone();

        Self {
            api_key,
            url,
            ws: None,
            debug: settings.debug,
            event_sender,
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new())),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.ws.is_some()
    }

    pub fn log(&self, msg: &str) {
        debug!("{}", msg);
    }

    pub async fn connect(&mut self, model: Option<&str>) -> anyhow::Result<()> {
        if self.api_key.is_none() && self.url == "wss://api.openai.com/v1/realtime" {
            warn!("No apiKey provided for connection to \"{}\"", self.url);
        }

        if self.is_connected() {
            return Err(anyhow!("Already connected"));
        }

        debug!("connecting...");

        let model = model.unwrap_or("gpt-4o-realtime-preview-2024-10-01");
        let url = format!("{}?model={}", self.url, model);

        let mut request = url.clone().into_client_request().unwrap();
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.api_key.as_ref().unwrap())
                .parse()
                .unwrap(),
        );
        request
            .headers_mut()
            .insert("OpenAI-Beta", format!("realtime=v1").parse().unwrap());

        // Connect to WebSocket
        let (ws_stream, _) = connect_async(request).await?;

        self.log(&format!("Connected to \"{}\"", url));

        // Set up message handling
        let (write, mut read) = ws_stream.split();
        let event_sender = Arc::new(Mutex::new(self.event_sender.clone()));
        let debug = self.debug;

        self.ws = Some(write);

        // Spawn a task to handle incoming messages
        tokio::spawn(async move {
            debug!("starting Websocket listener...");

            while let Some(message) = read.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        trace!("Received: {:#?}", serde_json::from_str::<Value>(&text));

                        if let Ok(event) = serde_json::from_str::<ApiEvent<Value>>(&text) {
                            info!("received {}", event.event_type);

                            let sender = event_sender.lock().await;

                            sender.send(Event::ServerEvent(event)).unwrap();
                        } else {
                            // error!("failed to deserialize ApiEvent from {}", &text);
                            panic!("failed to deserialize ApiEvent from {}", &text);
                        }
                    }
                    Ok(Message::Close(_)) => {
                        warn!("websocket was closed");
                        // Handle disconnection
                        let sender = event_sender.lock().await;
                        sender.send(Event::Close { error: false }).unwrap();
                        break;
                    }
                    Ok(Message::Ping(data)) => {
                        let sender = event_sender.lock().await;
                        sender.send(Event::Ping { data }).unwrap();
                    }
                    Ok(msg) => {
                        warn!("received unsupported message from websocket: {:?}", msg);
                    }
                    Err(e) => {
                        warn!("Error in WebSocket message: {:?}", e);
                        // Handle disconnection
                        let sender = event_sender.lock().await;
                        sender.send(Event::Close { error: true }).unwrap();
                        break;
                    }
                }
            }

            // Handle normal closure
            let sender = event_sender.lock().await;
            sender.send(Event::Close { error: false }).unwrap();
        });

        Ok(())
    }

    pub async fn disconnect(&mut self) {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close().await;
            self.log("Disconnected");
        }
    }

    pub async fn pong(&mut self, data: Vec<u8>) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Err(anyhow!("RealtimeAPI is not connected"));
        }

        if let Some(ws) = self.ws.as_mut() {
            ws.send(Message::Pong(data)).await?;
        }

        Ok(())
    }

    pub async fn send<T: Serialize + Debug + Clone>(
        &mut self,
        event_name: &str,
        data: Option<T>,
    ) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Err(anyhow!("RealtimeAPI is not connected"));
        }

        // Acquire a token from the rate limiter
        self.rate_limiter.lock().await.acquire().await;

        let event = ApiEvent {
            event_id: RealtimeUtils::generate_id("evt_", 16),
            event_type: event_name.to_string(),
            data: match data {
                Some(v) => serde_json::to_value(v)?.into(),
                None => None,
            },
        };

        if let Some(ws) = self.ws.as_mut() {
            let event_json = serde_json::to_string(&event)?;

            ws.send(Message::Text(event_json.clone())).await?;

            info!("sent event: {}", event.event_type);

            trace!("Sent: {:#?}", serde_json::to_value(event.clone()).unwrap());

            self.event_sender
                .send(Event::ClientEvent {
                    event_type: event_name.to_string(),
                    event_data: event.clone(),
                })
                .unwrap();

            Ok(())
        } else {
            Err(anyhow!("WebSocket not available"))
        }
    }

    pub async fn wait_for_next(
        &self,
        event_name: &str,
        timeout: Option<u64>,
    ) -> Option<serde_json::Value> {
        // This method should be implemented in the RealtimeClient instead
        unimplemented!("wait_for_next should be implemented in RealtimeClient")
    }
}
