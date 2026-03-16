use crate::types::{ContentPart, FormattedProperty, FormattedTool, TextContent};
use crate::utils::RealtimeUtils;
use log::debug;
use serde::de::value::MapDeserializer;
use serde::de::{Error, MapAccess, Visitor};
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Item {
    #[serde(flatten)]
    pub base: BaseItem,
    #[serde(flatten)]
    #[serde(skip_serializing)]
    pub formatted: FormattedItem,
}

impl Item {
    pub fn new(base: BaseItem) -> Self {
        Self {
            base,
            formatted: FormattedItem {
                id: RealtimeUtils::generate_id("item_", 16),
                object: "conversation.item".to_string(),
                // todo: auto take
                previous_item_id: None,
                formatted: FormattedProperty::default(),
            },
        }
    }

    pub fn append_transcript(&mut self, content_idx: usize, delta: &str) {
        self.base.append_transcript(content_idx, delta);
        self.formatted.append_transcript(delta);
    }

    pub fn set_content_transcript(&mut self, content_idx: usize, delta: &str) {
        self.base.set_content_transcript(content_idx, delta);
    }

    pub fn append_formatted_audio(&mut self, audio: Vec<i16>) {
        self.formatted.formatted.audio.extend(audio.iter());
    }

    pub fn set_formatted_audio(&mut self, audio: Vec<i16>) {
        self.formatted.formatted.audio = audio;
    }

    pub fn set_formatted_text(&mut self, text: String) {
        self.formatted.formatted.text = text;
    }

    pub fn set_formatted_transcript(&mut self, transcript: String) {
        self.formatted.formatted.transcript = transcript;
    }

    pub fn set_formatted_tool(&mut self, tool: FormattedTool) {
        self.formatted.formatted.tool = Some(tool);
    }

    pub fn set_formatted_output(&mut self, output: String) {
        self.formatted.formatted.output = Some(output);
    }

    pub fn get_type(&self) -> BaseItemType {
        match &self.base {
            BaseItem::System(item) => item.item_type,
            BaseItem::User(item) => item.item_type,
            BaseItem::Assistant(item) => item.item_type,
            BaseItem::FunctionCall(item) => item.item_type,
            BaseItem::FunctionCallOutput(item) => item.item_type,
        }
    }

    pub fn get_role(&self) -> Option<Role> {
        match &self.base {
            BaseItem::System(item) => Some(item.role),
            BaseItem::User(item) => Some(item.role),
            BaseItem::Assistant(item) => Some(item.role),
            BaseItem::FunctionCall(_) => None,
            BaseItem::FunctionCallOutput(_) => None,
        }
    }

    pub fn status(&self) -> Option<ItemStatus> {
        match &self.base {
            BaseItem::System(item) => Some(item.status),
            BaseItem::User(item) => Some(item.status),
            BaseItem::Assistant(item) => Some(item.status),
            BaseItem::FunctionCall(_) => None,
            BaseItem::FunctionCallOutput(_) => None,
        }
    }

    pub async fn play_audio(&self) -> anyhow::Result<()> {
        RealtimeUtils::play_samples(&self.formatted.formatted.audio).await
    }
}

/// Contains text and audio information about an item
/// Can also be used as a delta
#[derive(Default, Clone, Debug)]
pub struct ItemContentDelta {
    pub text: Option<String>,
    pub audio: Option<Vec<i16>>,
    pub arguments: Option<String>,
    pub transcript: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    Assistant,
    User,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    InProgress,
    Completed,
    Incomplete,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemItem {
    // pub previous_item_id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: BaseItemType,
    pub status: ItemStatus,
    pub role: Role,
    pub content: Vec<TextContent>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    InputAudio,
    Audio,
    InputText,
    Text,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserItem {
    // pub previous_item_id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: BaseItemType,
    pub status: ItemStatus,
    pub role: Role,
    pub content: Vec<ContentPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantItem {
    // pub previous_item_id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: BaseItemType,
    pub status: ItemStatus,
    pub role: Role,
    pub content: Vec<ContentPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FunctionItemType {
    FunctionCall,
    FunctionCallOutput,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCallItem {
    // pub previous_item_id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: BaseItemType,
    pub status: ItemStatus,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCallOutputItem {
    #[serde(rename = "type")]
    pub item_type: BaseItemType,
    pub call_id: String,
    pub output: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FormattedItem {
    pub id: String,
    pub object: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_item_id: Option<String>,
    #[serde(default)]
    pub formatted: FormattedProperty,
}

impl FormattedItem {
    pub fn append_transcript(&mut self, delta: &str) {
        self.formatted.transcript += delta;
    }
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum BaseItem {
    Assistant(AssistantItem),
    User(UserItem),
    System(SystemItem),
    FunctionCall(FunctionCallItem),
    FunctionCallOutput(FunctionCallOutputItem),
}

#[derive(Debug, Serialize, Clone, Deserialize, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BaseItemType {
    FunctionCall,
    FunctionCallOutput,
    Message,
}

// Implement custom Deserialize for BaseItemType
impl<'de> Deserialize<'de> for BaseItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BaseItemTypeVisitor;

        impl<'de> Visitor<'de> for BaseItemTypeVisitor {
            type Value = BaseItem;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a BaseItemType variant")
            }

            fn visit_map<V>(self, mut map: V) -> Result<BaseItem, V::Error>
            where
                V: MapAccess<'de>,
            {
                // Step 1: Collect the map into a HashMap
                let mut collected_map: HashMap<String, serde_json::Value> = HashMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    let value: serde_json::Value = map.next_value()?;
                    collected_map.insert(key, value);
                }

                // Step 2: Extract 'role' and 'item_type' from the HashMap
                let role = collected_map
                    .get("role")
                    .and_then(|r| serde_json::from_value(r.clone()).ok());
                let item_type = collected_map
                    .get("type")
                    .and_then(|r| serde_json::from_value(r.clone()).ok());

                debug!(
                    "deserializing BaseItemType with role={:?}, item_type={:?}",
                    &role, &item_type
                );

                // Step 3: Wrap the HashMap in a MapDeserializer
                let map_deserializer = MapDeserializer::new(collected_map.into_iter());

                // Step 4: Match the role and item_type and deserialize inner types
                match (role, item_type) {
                    (Some(Role::Assistant), _) => {
                        debug!("deserializing assistant from map");
                        Ok(BaseItem::Assistant(
                            AssistantItem::deserialize(map_deserializer).unwrap(),
                        ))
                    }
                    (Some(Role::User), _) => Ok(BaseItem::User(
                        UserItem::deserialize(map_deserializer).unwrap(),
                    )),
                    (Some(Role::System), _) => Ok(BaseItem::System(
                        SystemItem::deserialize(map_deserializer).unwrap(),
                    )),
                    (None, Some(BaseItemType::FunctionCall)) => Ok(BaseItem::FunctionCall(
                        FunctionCallItem::deserialize(map_deserializer).unwrap(),
                    )),
                    (None, Some(BaseItemType::FunctionCallOutput)) => {
                        Ok(BaseItem::FunctionCallOutput(
                            FunctionCallOutputItem::deserialize(map_deserializer).unwrap(),
                        ))
                    }
                    _ => Err(de::Error::custom("Invalid BaseItemType")),
                }
            }
        }

        deserializer.deserialize_map(BaseItemTypeVisitor)
    }
}

impl BaseItem {
    pub fn set_status(&mut self, status: ItemStatus) {
        match self {
            BaseItem::System(item) => item.status = status,
            BaseItem::User(item) => item.status = status,
            BaseItem::Assistant(item) => item.status = status,
            BaseItem::FunctionCall(item) => item.status = status,
            BaseItem::FunctionCallOutput(_) => {}
        }
    }

    pub fn status(&self) -> ItemStatus {
        match self {
            BaseItem::System(item) => (item.status),
            BaseItem::User(item) => (item.status),
            BaseItem::Assistant(item) => (item.status),
            BaseItem::FunctionCall(item) => (item.status),
            BaseItem::FunctionCallOutput(_) => ItemStatus::Completed,
        }
    }

    pub fn append_transcript(&mut self, content_index: usize, delta: &str) {
        match self {
            BaseItem::System(item) => {
                if let Some(content) = item.content.get_mut(content_index) {
                    content.text += &delta;
                }
            }
            BaseItem::User(item) => {
                if let Some(ContentPart::Text(content)) = item.content.get_mut(content_index) {
                    content.text += &delta;
                }
            }
            BaseItem::Assistant(item) => {
                if let Some(ContentPart::Text(content)) = item.content.get_mut(content_index) {
                    content.text += &delta;
                }
            }
            _ => {}
        }
    }

    pub fn set_content_transcript(&mut self, content_index: usize, delta: &str) {
        match self {
            BaseItem::System(item) => {
                if let Some(content) = item.content.get_mut(content_index) {
                    content.text = delta.to_string();
                }
            }
            BaseItem::User(item) => {
                if let Some(ContentPart::Text(content)) = item.content.get_mut(content_index) {
                    content.text = delta.to_string();
                }
            }
            BaseItem::Assistant(item) => {
                if let Some(ContentPart::Text(content)) = item.content.get_mut(content_index) {
                    content.text = delta.to_string();
                }
            }
            _ => {}
        }
    }

    pub fn is_complete(&self) -> bool {
        self.status() == ItemStatus::Completed
    }

    pub fn content_part_count(&self) -> usize {
        match self {
            BaseItem::System(item) => item.content.len(),
            BaseItem::User(item) => item.content.len(),
            BaseItem::Assistant(item) => item.content.len(),
            _ => 0,
        }
    }
}
