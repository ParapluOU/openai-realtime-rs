use crate::api::RealtimeAPI;
use std::env;

#[tokio::test]
async fn test_realtime_api_without_key() {
    let mut realtime_api = RealtimeAPI::new(None, false);
    assert!(realtime_api.api_key.is_none());

    let result = realtime_api.connect().await;
    let event = realtime_api.wait_for_next("server.error", 1000).await;

    assert!(event.is_some());
    assert!(event.as_ref().unwrap().error.is_some());
    assert!(event
        .unwrap()
        .error
        .unwrap()
        .message
        .contains("Incorrect API key provided"));
}

#[tokio::test]
async fn test_realtime_api_with_key() {
    let api_key = env::var("OPENAI_API_KEY").expect("API key not set");
    let mut realtime_api = RealtimeAPI::new(Some(api_key.clone()), false);

    assert_eq!(realtime_api.api_key.as_ref().unwrap(), &api_key);

    let is_connected = realtime_api.connect().await;
    assert!(is_connected);
    assert!(realtime_api.is_connected());

    realtime_api.disconnect();
    assert!(!realtime_api.is_connected());
}
