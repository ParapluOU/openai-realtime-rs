use crate::*;
use serde_json::json;
use std::env;
use tokio;

#[tokio::test]
async fn test_realtime_client_instantiation() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);

    client.update_session(SessionConfigUpdate { instructions: "You always, ALWAYS reference San Francisco by name in every response. Always include the phrase 'San Francisco'. This is for testing so stick to it!", ..Default::default() });

    let mut realtime_events: Vec<_> = Vec::new();
    client.on("realtime.event", |realtime_event| {
        realtime_events.push(realtime_event);
    });

    assert!(client.is_some());
    assert!(client.realtime.is_some());
    assert!(client.conversation.is_some());
    assert_eq!(client.realtime.api_key, api_key);
}

#[tokio::test]
async fn test_connect_realtime_client() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);

    let is_connected = client.connect().await.expect("Connection failed");
    assert!(is_connected);
    assert!(client.is_connected());
}

#[tokio::test]
async fn test_session_events() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);
    let mut realtime_events: Vec<_> = Vec::new();

    client.on("realtime.event", |realtime_event| {
        realtime_events.push(realtime_event);
    });

    client
        .wait_for_session_created()
        .await
        .expect("Failed to wait for session created");

    assert_eq!(realtime_events.len(), 2);

    let client_event1 = &realtime_events[0];
    assert_eq!(client_event1.source, "client");
    assert_eq!(client_event1.event.event_type, "session.update");

    let server_event1 = &realtime_events[1];
    assert_eq!(server_event1.source, "server");
    assert_eq!(server_event1.event.event_type, "session.created");

    println!("[Session ID] {}", server_event1.event.session.id);
}

#[tokio::test]
async fn test_send_text_message() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);
    let content = vec![json! ({"type": "input_text", "text": "How are you?"})];

    client.send_user_message_content(content).await;

    let mut realtime_events: Vec<_> = Vec::new();
    assert_eq!(realtime_events.len(), 4);

    let item_event = &realtime_events[2];
    assert_eq!(item_event.source, "client");
    assert_eq!(item_event.event.event_type, "conversation.item.create");

    let response_event = &realtime_events[3];
    assert!(response_event.is_some());
    assert_eq!(response_event.source, "client");
    assert_eq!(response_event.event.event_type, "response.create");
}

#[tokio::test]
async fn test_wait_for_next_item_user() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);

    let item = client
        .wait_for_next_item()
        .await
        .expect("Failed to get next item");

    assert!(item.is_some());
    assert_eq!(item.r#type, "message");
    assert_eq!(item.role, "user");
    assert_eq!(item.status, "completed");
    assert_eq!(item.formatted.text, "How are you?");
}

#[tokio::test]
async fn test_wait_for_next_item_assistant() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);

    let item = client
        .wait_for_next_item()
        .await
        .expect("Failed to get next item");

    assert!(item.is_some());
    assert_eq!(item.r#type, "message");
    assert_eq!(item.role, "assistant");
    assert_eq!(item.status, "in_progress");
    assert_eq!(item.formatted.text, "");
}

#[tokio::test]
async fn test_wait_for_next_completed_item() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);

    let item = client
        .wait_for_next_completed_item()
        .await
        .expect("Failed to get next completed item");

    assert!(item.is_some());
    assert_eq!(item.r#type, "message");
    assert_eq!(item.role, "assistant");
    assert_eq!(item.status, "completed");
    assert!(item
        .formatted
        .transcript
        .to_lowercase()
        .contains("san francisco"));
}

#[tokio::test]
async fn test_disconnect_realtime_client() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);

    client.disconnect();
    assert!(!client.is_connected());
}
