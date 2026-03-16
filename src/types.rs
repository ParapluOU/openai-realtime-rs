use crate::item::{AssistantItem, BaseItem, BaseItemType, ContentType, Item, ItemStatus, Role};
use crate::utils::RealtimeUtils;
use enum_variant_macros::FromVariants;
use serde::de::Visitor;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::path::Path;

#[derive(Clone)]
pub struct QueuedSpeech {
    pub(crate) audio_start_ms: u64,
    pub(crate) audio_end_ms: Option<u64>,
    pub(crate) audio: Option<Vec<i16>>,
}

#[derive(Clone)]
pub struct QueuedTranscript {
    pub(crate) transcript: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TextContent {
    #[serde(rename = "type")]
    pub content_type: ContentType,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AudioContent {
    #[serde(rename = "type")]
    pub content_type: ContentType,
    /// empty if we get it back as confirmation that it was processed
    pub audio: Option<String>,
    #[serde(default)]
    pub transcript: Option<String>,
}

impl AudioContent {
    pub fn from_audio(audio: &[i16]) -> Self {
        let audio_base64 = RealtimeUtils::array_buffer_to_base64(audio);

        Self {
            content_type: ContentType::InputAudio,
            audio: audio_base64.into(),
            transcript: None,
        }
    }

    pub fn from_audio_file(path: &Path) -> anyhow::Result<Self> {
        Ok(Self::from_audio(&RealtimeUtils::samples_from_file(path)?))
    }
}

// #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
// #[serde(untagged)]
// pub enum ContentType {
//     Audio(AudioContentType),
//     Text(TextContentType),
// }

pub type AssistantContentType = ContentPart;
pub type UserContentType = ContentPart;

#[test]
fn test_deserialize_assistant_item_type() {
    let json_str = r#"{
            "content": [],
            "id": "item_AFFzrIDWgPxbuwbHaNVp5",
            "object": "realtime.item",
            "role": "assistant",
            "status": "in_progress",
            "type": "message"
        }"#;

    let assistant_item: AssistantItem = serde_json::from_str(json_str).unwrap();

    // assert_eq!(assistant_item.previous_item_id, None);
    assert_eq!(assistant_item.item_type, BaseItemType::Message);
    assert_eq!(assistant_item.status, ItemStatus::InProgress);
    assert_eq!(assistant_item.role, Role::Assistant);
    assert_eq!(assistant_item.content, Vec::<ContentPart>::new());

    let assistant_item: BaseItem = serde_json::from_str(json_str).unwrap();
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FormattedTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub call_id: String,
    pub arguments: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct FormattedProperty {
    pub audio: Vec<i16>,
    pub text: String,
    pub transcript: String,
    pub tool: Option<FormattedTool>,
    pub output: Option<String>,
    pub file: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum IncompleteResponseStatusType {
    #[serde(rename = "incomplete")]
    Incomplete { reason: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub struct CancelledResponseStatusType {
    reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum FailedResponseStatusType {
    #[serde(rename = "failed")]
    Failed { error: Option<serde_json::Value> },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UsageType {
    pub total_tokens: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ResponseStatusDetails {
    Cancelled(CancelledResponseStatusType),
    Incomplete(IncompleteResponseStatusType),
    Failed(FailedResponseStatusType),
}

#[test]
fn test_deserialize_cancelled_response_status() {
    let json_str = r#"{
        "reason": "turn_detected",
        "type": "cancelled"
    }"#;

    let status: ResponseStatusDetails = serde_json::from_str(json_str).unwrap();

    match status {
        ResponseStatusDetails::Cancelled(CancelledResponseStatusType { reason }) => {
            assert_eq!(reason, "turn_detected");
        }
        _ => panic!("Expected CancelledResponseStatusType"),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseResourceType {
    pub id: String,
    pub object: String,
    pub status: String,
    pub status_details: Option<ResponseStatusDetails>,
    // pub output: Vec<ItemType>,
    pub output: Vec<Item>,
    pub usage: Option<UsageType>,
}

// Add the following new types

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, FromVariants)]
#[serde(untagged)]
pub enum ContentPart {
    Text(TextContent),
    Audio(AudioContent),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationItemTruncate {
    pub item_id: String,
    pub audio_end_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiEvent<T: Debug + Clone> {
    pub event_id: String,
    #[serde(rename = "type")]
    pub event_type: String,

    #[serde(flatten)]
    pub data: Option<T>,
}

/// Object {
///     "content_index": Number(0),
///     "event_id": String("event_AFLIObKF4AGkQiGFY7ctU"),
///     "item_id": String("item_AFLIOnwBBdFBsn71KWrXP"),
///     "output_index": Number(0),
///     "response_id": String("resp_AFLIO1VY9Khvy3ulGo58q"),
///     "type": String("response.audio.done"),
// }
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseAudioDoneEvent {
    pub event_id: String,
    pub content_index: usize,
    pub item_id: String,
    pub output_index: u32,
    pub response_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseAudioTranscriptDoneEvent {
    pub event_id: String,
    pub content_index: usize,
    pub item_id: String,
    pub output_index: u32,
    pub response_id: String,
    pub transcript: String,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseContentPartDoneEvent {
    pub event_id: String,
    pub content_index: usize,
    pub item_id: String,
    pub output_index: u32,
    pub response_id: String,
    pub part: ContentPart,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseOutputItemDoneEvent {
    pub event_id: String,
    // pub content_index: usize,
    // pub item_id: String,
    pub output_index: u32,
    pub response_id: String,
    pub item: Item,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseAudioDeltaEvent {
    // pub content_index: usize,
    // pub item_id: String,
    pub output_index: u32,
    pub content_index: u32,
    pub response_id: String,
    pub item_id: String,
    /// base64 encoded PCM16 samples
    pub delta: String,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RateLimitsUpdatedEventItem {
    limit: u32,
    name: String,
    remaining: u32,
    reset_seconds: f32,
}
