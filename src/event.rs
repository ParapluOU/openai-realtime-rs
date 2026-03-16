use crate::item::Item;
use crate::session::SessionConfig;
use crate::types::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "session.created")]
    SessionCreated {
        session: SessionConfig,
    },

    #[serde(rename = "session.updated")]
    SessionUpdated {
        session: SessionConfig,
    },

    #[serde(rename = "conversation.item.created")]
    ConversationItemCreated {
        item: Item,
    },

    #[serde(rename = "conversation.item.appended")]
    ConversationItemAppended {
        item: Item,
    },

    #[serde(rename = "conversation.item.truncated")]
    ConversationItemTruncated {
        item_id: String,
        audio_end_ms: u64,
    },

    #[serde(rename = "conversation.item.deleted")]
    ConversationItemDeleted {
        item_id: String,
    },

    #[serde(rename = "conversation.item.input_audio_transcription.completed")]
    ConversationItemInputAudioTranscriptionCompleted {
        item_id: String,
        content_index: usize,
        transcript: String,
    },

    #[serde(rename = "input_audio_buffer.append")]
    InputAudioBufferAppend {
        audio: String,
    },

    #[serde(rename = "input_audio_buffer.committed")]
    InputAudioBufferCommitted,

    #[serde(rename = "input_audio_buffer.speech_started")]
    InputAudioBufferSpeechStarted {
        item_id: String,
        audio_start_ms: u64,
    },

    #[serde(rename = "input_audio_buffer.speech_stopped")]
    InputAudioBufferSpeechStopped {
        item_id: String,
        audio_end_ms: u64,
    },

    /// client request for generating a response
    #[serde(rename = "response.create")]
    ResponseCreateRequest,

    #[serde(rename = "response.created")]
    ResponseCreated {
        response: ResponseResourceType,
    },

    #[serde(rename = "response.done")]
    ResponseDone {
        event_id: String,
        response: ResponseResourceType,
    },

    #[serde(rename = "response.output_item.added")]
    ResponseOutputItemAdded {
        response_id: String,
        output_index: u32,
        item: Item,
    },

    #[serde(rename = "response.output_item.done")]
    ResponseOutputItemDone(ResponseOutputItemDoneEvent),

    #[serde(rename = "response.content_part.done")]
    ResponseContentPartDone(ResponseContentPartDoneEvent),

    #[serde(rename = "response.content_part.added")]
    ResponseContentPartAdded {
        item_id: String,
        part: ContentPart,
    },

    #[serde(rename = "response.audio_transcript.delta")]
    ResponseAudioTranscriptDelta {
        item_id: String,
        content_index: usize,
        delta: String,
    },

    #[serde(rename = "response.audio_transcript.done")]
    ResponseAudioTranscriptDone(ResponseAudioTranscriptDoneEvent),

    #[serde(rename = "response.audio.delta")]
    ResponseAudioDelta(ResponseAudioDeltaEvent),

    #[serde(rename = "response.audio.done")]
    ResponseAudioDone(ResponseAudioDoneEvent),

    #[serde(rename = "response.text.delta")]
    ResponseTextDelta {
        item_id: String,
        content_index: usize,
        delta: String,
    },

    /// pretty worthless as we are receiving increments of a JSON object which is unparseable
    #[serde(rename = "response.function_call_arguments.delta")]
    ResponseFunctionCallArgumentsDelta {
        item_id: String,
        delta: String,
    },

    #[serde(rename = "response.function_call_arguments.done")]
    ResponseFunctionCallArgumentsDone {
        item_id: String,
        /// function name
        name: String,
        /// json object embedded in String
        arguments: String,

        output_index: usize,
        call_id: String,
    },

    /// todo: update RateLimiter class with this info
    #[serde(rename = "rate_limits.updated")]
    RateLimitsUpdated {
        event_id: String,
        rate_limits: Vec<RateLimitsUpdatedEventItem>,
    },

    ClientEvent {
        event_type: String,
        event_data: ApiEvent<serde_json::Value>,
    },
    ServerEvent(ApiEvent<serde_json::Value>),

    /// request to return health check to server
    Ping {
        data: Vec<u8>,
    },

    /// websocket closed from server side
    Close {
        error: bool,
    },
}

impl Event {
    /// Returns the serde tag (event type) as a String
    /// todo: make strongly typed enum
    pub fn get_type(&self) -> String {
        match self {
            Event::SessionCreated { .. } => "session.created",
            Event::SessionUpdated { .. } => "session.updated",
            Event::ConversationItemCreated { .. } => "conversation.item.created",
            Event::ConversationItemAppended { .. } => "conversation.item.appended",
            Event::ConversationItemTruncated { .. } => "conversation.item.truncated",
            Event::ConversationItemDeleted { .. } => "conversation.item.deleted",
            Event::ConversationItemInputAudioTranscriptionCompleted { .. } => {
                "conversation.item.input_audio_transcription.completed"
            }
            Event::InputAudioBufferAppend { .. } => "input_audio_buffer.append",
            Event::InputAudioBufferCommitted => "input_audio_buffer.committed",
            Event::InputAudioBufferSpeechStarted { .. } => "input_audio_buffer.speech_started",
            Event::InputAudioBufferSpeechStopped { .. } => "input_audio_buffer.speech_stopped",
            Event::ResponseCreateRequest => "response.create",
            Event::ResponseCreated { .. } => "response.created",
            Event::ResponseDone { .. } => "response.done",
            Event::ResponseOutputItemAdded { .. } => "response.output_item.added",
            Event::ResponseOutputItemDone(_) => "response.output_item.done",
            Event::ResponseContentPartDone(_) => "response.content_part.done",
            Event::ResponseContentPartAdded { .. } => "response.content_part.added",
            Event::ResponseAudioTranscriptDelta { .. } => "response.audio_transcript.delta",
            Event::ResponseAudioTranscriptDone(_) => "response.audio_transcript.done",
            Event::ResponseAudioDelta(_) => "response.audio.delta",
            Event::ResponseAudioDone(_) => "response.audio.done",
            Event::ResponseTextDelta { .. } => "response.text.delta",
            Event::ResponseFunctionCallArgumentsDelta { .. } => {
                "response.function_call_arguments.delta"
            }
            Event::RateLimitsUpdated { .. } => "rate_limits.updated",
            Event::ClientEvent { event_type, .. } => event_type,
            Event::ServerEvent(api_event) => &api_event.event_type,
            Event::Ping { .. } => "ping",
            Event::Close { .. } => "close",
            Event::ResponseFunctionCallArgumentsDone { .. } => {
                "response.function_call_arguments.done"
            }
        }
        .to_string()
    }
}

pub type EventSender = broadcast::Sender<Event>;
pub type EventReceiver = broadcast::Receiver<Event>;

#[test]
fn test_deserialize_item_added() {
    let event = ApiEvent {
        event_id: "event_AEu8aEWc0jb5G5Bi8T1EW".to_string(),
        event_type: "response.output_item.added".to_string(),
        data: Some(json!( {
            "item": {
                "content": [],
                "id": "item_AEu8ZZ8loE5j9hLaW3VYD",
                "object": ("realtime.item"),
                "role": ("assistant"),
                "status": ("in_progress"),
                "type": ("message"),
            },
            "output_index": 0,
            "response_id": ("resp_AEu8Z8hMBc7k7GI2Z5L6y"),
        })),
    };

    let reinterpreted: Event =
        serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap();
}
