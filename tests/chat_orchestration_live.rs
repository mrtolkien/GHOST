#![cfg(feature = "live-tests")]

mod common;

use ghost::chat::ChatStopReason;

#[tokio::test]
async fn session_chat_live_roundtrip_with_default_config() {
    let env = common::live_test_database("chat_live_roundtrip").await;
    let session = env.create_session().await;

    let chat = env.chat();
    let (result, _metadata) = chat
        .chat(
            &session,
            "Reply in one short sentence: what is Rust best known for?",
            None,
            None,
        )
        .await
        .expect("chat response");

    env.log_session_json("chat", &session).await;

    assert!(!result.message.trim().is_empty());
    assert!(matches!(
        result.stop_reason,
        ChatStopReason::EndTurn | ChatStopReason::MaxTokens
    ));

    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list messages");
    assert!(
        messages.iter().any(|msg| msg.role == "user"),
        "expected persisted user message"
    );
    assert!(
        messages.iter().any(|msg| msg.role == "assistant"),
        "expected persisted assistant message"
    );
}
