use crate::session::ToolDefinition;
use serde_json::Value;
use std::sync::Arc;

/// Represents a handler for a tool.
#[derive(Clone)]
pub struct ToolHandler {
    pub(crate) definition: ToolDefinition,
    pub(crate) handler: Arc<tokio::sync::Mutex<dyn Fn(Value) -> anyhow::Result<Value> + Send>>,
}
