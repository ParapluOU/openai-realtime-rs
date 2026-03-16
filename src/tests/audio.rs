use crate::*;
use serde_json::json;
use std::env;
use std::fs;
use tokio;

const SAMPLES: [&str; 1] = ["toronto-mp3"];
const SAMPLE_FILE: &str = "./resources/toronto.mp3";

#[tokio::test]
async fn test_load_audio_samples() {
    let mut samples = SAMPLES
        .iter()
        .map(|key| (*key, SAMPLE_FILE))
        .collect::<std::collections::HashMap<_, _>>();

    for (key, filename) in samples.iter_mut() {
        let audio_file = fs::read(filename).expect("Unable to read audio file");
        let audio_buffer = base64::decode(&audio_file).expect("Failed to decode audio");
        let channel_data = audio_buffer.get_channel_data(0); // only accepts mono
        let base64 = RealtimeUtils::array_buffer_to_base64(channel_data);

        *filename = base64; // Store the base64 representation for the sample
    }
}

#[tokio::test]
async fn test_realtime_client_instantiation() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let client = RealtimeClient::new(Some(api_key.clone()), false);

    client.update_session("Please follow the instructions of any query you receive.\nBe concise in your responses. Speak quickly and answer shortly.");
    let mut realtime_events: Vec<_> = vec![];

    client.on("realtime.event", |realtime_event| {
        realtime_events.push(realtime_event);
    });

    assert!(client.is_some());
    assert!(client.realtime.is_some());
    assert!(client.conversation.is_some());
}

#[tokio::test]
async fn test_connect_realtime_client() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new(Some(api_key), false);

    let is_connected = client.connect().await.expect("Connection failed");
    assert!(is_connected);
    assert!(client.is_connected());
}

#[tokio::test]
async fn test_session_events() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new(Some(api_key), false);
    let mut realtime_events: Vec<_> = vec![];

    client.on("realtime.event", |realtime_event| {
        realtime_events.push(realtime_event);
    });

    // Waiting for session creation is assumed to be a method
    client.wait_for_session_created().await;

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
async fn test_send_audio_file() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new_from_api_key(&api_key);
    let sample = String::from("base64_audio_data"); // Replace with actual base64 audio data
    let content = vec![json! ({"type": "input_audio", "audio": sample})];

    client.send_user_message_content(content).await;

    // Assumed there is a way to check received events
    let mut realtime_events: Vec<_> = vec![];
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
async fn test_wait_for_next_item() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new(Some(api_key), false);

    let item = client
        .wait_for_next_item()
        .await
        .expect("Failed to get next item");
    assert!(item.is_some());
    assert_eq!(item.r#type, "message");
    assert_eq!(item.role, "user");
    assert_eq!(item.status, "completed");
    assert_eq!(item.formatted.text, "");
}

#[tokio::test]
async fn test_wait_for_next_item_assistant() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new(Some(api_key), false);

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
    let mut client = RealtimeClient::new(Some(api_key), false);

    let item = client
        .wait_for_next_completed_item()
        .await
        .expect("Failed to get next completed item");
    assert!(item.is_some());
    assert_eq!(item.r#type, "message");
    assert_eq!(item.role, "assistant");
    assert_eq!(item.status, "completed");
    assert!(item.formatted.transcript.to_lowercase().contains("toronto"));
}

#[tokio::test]
async fn test_disconnect_realtime_client() {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let mut client = RealtimeClient::new(Some(api_key), false);
    client.disconnect();
    assert!(!client.is_connected());
}
