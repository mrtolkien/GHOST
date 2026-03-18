mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ghost::chat::SessionChat;
use ghost::chat::interrupt;
use ghost::db;
use ghost::providers::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError, StopReason, Usage,
};
use ghost::tools::ToolManager;

// ---------------------------------------------------------------------------
// Test 1: SessionBusy returned when session already active
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_returns_session_busy_when_session_already_active() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;

    // Provider with no responses — chat() should never reach the provider.
    let provider = Arc::new(common::MockProvider::new(vec![]));
    let active_sessions: interrupt::ActiveSessions = Arc::new(dashmap::DashMap::new());

    let session_chat = SessionChat::new(
        db.clone(),
        provider,
        ToolManager::for_chat(),
        common::shared(&config),
    )
    .with_active_sessions(active_sessions.clone());

    let session_id = db::sessions::create_session(&db).await.unwrap();

    // Pre-insert to simulate a running tool loop.
    let (tx, _rx) = interrupt::channel();
    active_sessions.insert(session_id.clone(), tx);

    let result = session_chat.chat(&session_id, "hello", None, None).await;

    assert!(
        matches!(&result, Err(ghost::chat::ChatError::SessionBusy { .. })),
        "expected SessionBusy, got {result:?}"
    );

    // Verify no user message was written to DB.
    let messages = db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .unwrap();
    assert!(
        messages.is_empty(),
        "expected no messages written to DB, found {}",
        messages.len()
    );
}

// ---------------------------------------------------------------------------
// Test 2: Steer interrupts are drained on EndTurn
// ---------------------------------------------------------------------------

/// A provider that pauses its first chat() call, allowing the test to
/// inject a steer message into the interrupt channel before the tool loop
/// processes the EndTurn response.
#[derive(Debug)]
struct CoordinatedProvider {
    responses: Mutex<VecDeque<ChatResponse>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    first_call_started: Arc<tokio::sync::Notify>,
    steer_sent: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl Provider for CoordinatedProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        let call_number = {
            let mut reqs = self.requests.lock().expect("lock requests");
            reqs.push(request);
            reqs.len()
        };

        if call_number == 1 {
            self.first_call_started.notify_one();
            self.steer_sent.notified().await;
        }

        self.responses
            .lock()
            .expect("lock responses")
            .pop_front()
            .ok_or_else(|| ProviderError::InvalidResponse("no mock response remaining".into()))
    }

    fn name(&self) -> &str {
        "coordinated-mock"
    }
}

fn endturn_response(text: &str) -> ChatResponse {
    ChatResponse {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        usage: Usage::default(),
        stop_reason: StopReason::EndTurn,
        model: "mock".to_string(),
        response_id: None,
        turn_state: None,
    }
}

#[tokio::test]
async fn steer_interrupt_drained_on_end_turn_triggers_continuation() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;

    let first_call_started = Arc::new(tokio::sync::Notify::new());
    let steer_sent = Arc::new(tokio::sync::Notify::new());
    let requests = Arc::new(Mutex::new(Vec::new()));

    let provider = Arc::new(CoordinatedProvider {
        responses: Mutex::new(VecDeque::from(vec![
            endturn_response("first response"),
            endturn_response("second response after steer"),
        ])),
        requests: Arc::clone(&requests),
        first_call_started: Arc::clone(&first_call_started),
        steer_sent: Arc::clone(&steer_sent),
    });

    let active_sessions: interrupt::ActiveSessions = Arc::new(dashmap::DashMap::new());
    let session_chat = SessionChat::new(
        db.clone(),
        provider as Arc<dyn Provider>,
        ToolManager::for_chat(),
        common::shared(&config),
    )
    .with_active_sessions(active_sessions.clone());

    let session_id = db::sessions::create_session(&db).await.unwrap();

    // Spawn a task that waits for the first provider call, then sends a steer.
    let as_clone = active_sessions.clone();
    let sid_clone = session_id.clone();
    tokio::spawn(async move {
        first_call_started.notified().await;
        // Send steer into the interrupt channel via active_sessions.
        if let Some(tx) = as_clone.get(&sid_clone) {
            let _ = tx.send(interrupt::Interrupt::Steer {
                message: "steered message".to_string(),
            });
        }
        // Signal the provider to return its first response.
        steer_sent.notify_one();
    });

    let (result, _metadata) = session_chat
        .chat(&session_id, "initial message", None, None)
        .await
        .unwrap();

    // The model should have been called twice:
    // 1. First EndTurn -> drain finds steer -> continues
    // 2. Second EndTurn -> no interrupts -> exits
    let call_count = requests.lock().unwrap().len();
    assert_eq!(
        call_count, 2,
        "expected 2 provider calls (drain continued after steer), got {call_count}"
    );

    // Final response should be from the second call.
    assert_eq!(result.message, "second response after steer");

    // Verify the steered message was persisted to DB.
    let messages = db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .unwrap();
    let steer_msg = messages
        .iter()
        .find(|m| m.role == "user" && m.content == "steered message");
    assert!(
        steer_msg.is_some(),
        "expected steered message to be persisted to DB"
    );
}
