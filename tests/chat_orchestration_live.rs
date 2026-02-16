#![cfg(feature = "live-tests")]

use ghost::chat::{ChatStopReason, SessionChat};

#[tokio::test]
async fn session_chat_live_roundtrip_with_default_config() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");

    let config = ghost::config::load().expect("load config from ~/.config/ghost");
    ghost::config::bootstrap_workspace(&config).expect("bootstrap workspace");

    assert_provider_ready(&config).await;

    let db = ghost::db::connect(&config.workspace)
        .await
        .expect("connect db");
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let chat = SessionChat::from_config(db.clone(), config).expect("build session chat");
    let result = chat
        .chat(
            &session_id.to_string(),
            "Reply in one short sentence: what is Rust best known for?",
        )
        .await
        .expect("chat response");

    assert!(!result.message.trim().is_empty());
    assert!(matches!(
        result.stop_reason,
        ChatStopReason::EndTurn | ChatStopReason::MaxTokens
    ));

    let messages = ghost::db::sessions::list_messages_by_session(&db, &session_id)
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

async fn assert_provider_ready(config: &ghost::config::Config) {
    let model = config
        .models
        .aliases
        .get(&config.models.default)
        .expect("default model alias exists");

    match model.provider.as_str() {
        "openrouter" => {
            assert!(
                std::env::var("OPENROUTER_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .is_some(),
                "OPENROUTER_API_KEY must be set for live SessionChat test"
            );
        }
        "kimi_code" | "kimi" => {
            assert!(
                std::env::var("KIMI_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .is_some(),
                "KIMI_API_KEY must be set for live SessionChat test"
            );
        }
        "openai_oauth" => {
            assert!(
                ghost::auth::openai_oauth::auth_status()
                    .await
                    .expect("read oauth status")
                    .is_some(),
                "No OpenAI OAuth token found; run `ghost auth codex` first"
            );
        }
        other => {
            panic!("Unsupported provider '{other}' in default model");
        }
    }
}
