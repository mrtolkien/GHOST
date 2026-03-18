mod common;

use std::sync::Arc;

use ghost::chat::{ChatStopReason, SessionChat};
use ghost::providers::{ContentBlock, StopReason};
use ghost::tools::ToolManager;
use serde_json::json;

use common::{EchoTool, MockProvider, respond_response, response, shared};

#[tokio::test]
async fn chat_returns_response_text() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![respond_response(
        "hello from ghost",
        vec![],
    )]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let result = chat
        .chat(&session_id, "hi", None, None)
        .await
        .expect("chat result");

    assert_eq!(result.0.message, "hello from ghost");
    assert_eq!(result.0.stop_reason, ChatStopReason::EndTurn);
}

#[tokio::test]
async fn tool_loop_executes_and_sends_tool_result_back() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![
        response(
            vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "echo_tool".to_string(),
                input: json!({"text": "abc"}),
            }],
            StopReason::ToolUse,
        ),
        respond_response("done", vec![]),
    ]));
    let requests = provider.requests();

    let mut tools = ToolManager::empty();
    tools.register(Arc::new(EchoTool));
    let chat = SessionChat::new(db.clone(), provider, tools, shared(&config));

    let result = chat
        .chat(&session_id, "run tool", None, None)
        .await
        .expect("chat result");
    assert_eq!(result.0.message, "done");

    let recorded = requests.lock().expect("lock requests");
    assert_eq!(recorded.len(), 2);
    let second = &recorded[1];
    let has_tool_result = second.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    content,
                    is_error: false,
                    ..
                } if content == "echo:abc"
            )
        })
    });
    assert!(has_tool_result);
}

#[tokio::test]
async fn max_iterations_stops_loop() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![
        response(
            vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "echo_tool".to_string(),
                input: json!({"text": "x"}),
            }],
            StopReason::ToolUse,
        ),
        response(
            vec![ContentBlock::ToolUse {
                id: "call_2".to_string(),
                name: "echo_tool".to_string(),
                input: json!({"text": "y"}),
            }],
            StopReason::ToolUse,
        ),
    ]));

    let mut tools = ToolManager::empty();
    tools.register(Arc::new(EchoTool));
    let chat =
        SessionChat::new(db.clone(), provider, tools, shared(&config)).with_max_tool_iterations(1);
    let result = chat
        .chat(&session_id, "loop", None, None)
        .await
        .expect("chat result");
    assert_eq!(result.0.stop_reason, ChatStopReason::MaxIterations);
}

#[tokio::test]
async fn chat_persists_user_and_assistant_messages() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![respond_response(
        "persisted",
        vec![],
    )]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));

    let _ = chat
        .chat(&session_id, "persist", None, None)
        .await
        .expect("chat result");

    let messages = ghost::db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
}

#[tokio::test]
async fn reboot_marks_old_session_and_creates_new_one() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let old_session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    ghost::db::interface_sessions::set_active_session_for_interface(
        &db,
        "discord:channel:123",
        &old_session_id,
    )
    .await
    .expect("set mapping");

    let provider = Arc::new(MockProvider::new(vec![]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let new_session = chat
        .reboot_session(&old_session_id)
        .await
        .expect("reboot session");

    assert_ne!(new_session, old_session_id);

    let old_record = ghost::db::sessions::get_session(&db, &old_session_id)
        .await
        .expect("old session");
    assert_eq!(old_record.status, "rebooted");

    let active =
        ghost::db::interface_sessions::get_active_session_for_interface(&db, "discord:channel:123")
            .await
            .expect("active")
            .expect("active session");
    assert_eq!(active, new_session);
}

/// Main chat does NOT inject TODO state — nudges are Lua-only (agents).
#[tokio::test]
async fn main_chat_does_not_inject_todo() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    ghost::db::sessions::set_session_todo_list(
        &db,
        &session_id,
        Some(&[ghost::tools::TodoItem {
            title: "Step 1".to_string(),
            description: None,
            status: ghost::tools::TodoStatus::Pending,
            note: None,
        }]),
    )
    .await
    .expect("set todo");

    let provider = Arc::new(MockProvider::new(vec![respond_response("ok", vec![])]));
    let requests = provider.requests();
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let _ = chat
        .chat(&session_id, "check todo", None, None)
        .await
        .expect("chat result");

    let requests = requests.lock().expect("lock requests");
    let first = requests.first().expect("request");
    let has_todo = first.messages.iter().any(|message| {
        message.role == ghost::providers::Role::System
            && message.content.iter().any(
                |block| matches!(block, ContentBlock::Text { text } if text.contains("Current TODO")),
            )
    });
    assert!(!has_todo, "main chat should not inject TODO state");
}

// ---------------------------------------------------------------------------
// Orphaned tool-call repair tests
// ---------------------------------------------------------------------------

/// Simulate a crash: assistant message has tool_calls but no following
/// tool_results. On the next chat(), the history should contain a
/// synthetic error tool result.
#[tokio::test]
async fn orphaned_tool_calls_get_error_results() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    // Simulate: user message, then assistant with tool_calls, then crash
    // (no tool_results message).
    ghost::db::sessions::create_message(&db, &session_id, "user", "do something")
        .await
        .unwrap();
    ghost::db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "assistant",
        "",
        Some(vec![
            json!({"id": "call_orphan", "name": "echo_tool", "input": {"text": "x"}}),
        ]),
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // Now chat again — the repair should inject an error result.
    let provider = Arc::new(MockProvider::new(vec![respond_response(
        "recovered",
        vec![],
    )]));
    let requests = provider.requests();
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let result = chat
        .chat(&session_id, "continue", None, None)
        .await
        .expect("chat result");
    assert_eq!(result.0.message, "recovered");

    // The provider request should contain a ToolResult with is_error=true
    // for the orphaned call, positioned *before* the new user message.
    {
        let recorded = requests.lock().expect("lock");
        let req = &recorded[0];
        let error_result = req
            .messages
            .iter()
            .flat_map(|m| m.content.iter())
            .find(|b| {
                matches!(
                    b,
                    ContentBlock::ToolResult {
                        tool_use_id,
                        is_error: true,
                        ..
                    } if tool_use_id == "call_orphan"
                )
            });
        assert!(
            error_result.is_some(),
            "expected error tool result for orphaned call"
        );
    }

    // The error result should also be persisted in the DB.
    let messages = ghost::db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .unwrap();
    let repair_msg = messages.iter().find(|m| {
        m.tool_results
            .as_deref()
            .is_some_and(|r| r.contains("call_orphan"))
    });
    assert!(
        repair_msg.is_some(),
        "repair message should be persisted in DB"
    );
}

/// Partial crash: assistant called 3 tools, only 2 results were written.
/// The repair should only synthesize the missing one.
#[tokio::test]
async fn partial_tool_results_get_remaining_errors() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    ghost::db::sessions::create_message(&db, &session_id, "user", "run three tools")
        .await
        .unwrap();
    ghost::db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "assistant",
        "",
        Some(vec![
            json!({"id": "call_a", "name": "t", "input": {}}),
            json!({"id": "call_b", "name": "t", "input": {}}),
            json!({"id": "call_c", "name": "t", "input": {}}),
        ]),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    // Only results for A and B were written before crash.
    ghost::db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "user",
        "",
        None,
        Some(vec![
            json!({"tool_use_id": "call_a", "content": "ok", "is_error": false}),
            json!({"tool_use_id": "call_b", "content": "ok", "is_error": false}),
        ]),
        None,
        None,
    )
    .await
    .unwrap();

    let provider = Arc::new(MockProvider::new(vec![respond_response("ok", vec![])]));
    let requests = provider.requests();
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let _ = chat
        .chat(&session_id, "continue", None, None)
        .await
        .expect("chat result");

    let recorded = requests.lock().expect("lock");
    let req = &recorded[0];

    // Should have error result for call_c only.
    let error_results: Vec<_> = req
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter(|b| matches!(b, ContentBlock::ToolResult { is_error: true, .. }))
        .collect();
    assert_eq!(
        error_results.len(),
        1,
        "only call_c should get error result"
    );
    assert!(matches!(
        error_results[0],
        ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_c"
    ));

    // Normal results for A and B should still be present.
    let ok_results: Vec<_> = req
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter(|b| {
            matches!(
                b,
                ContentBlock::ToolResult {
                    is_error: false,
                    content,
                    ..
                } if content == "ok"
            )
        })
        .collect();
    assert_eq!(
        ok_results.len(),
        2,
        "call_a and call_b results should be present"
    );
}

/// When all tool results are present, no repair should happen.
#[tokio::test]
async fn complete_tool_results_not_repaired() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    ghost::db::sessions::create_message(&db, &session_id, "user", "run tool")
        .await
        .unwrap();
    ghost::db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "assistant",
        "calling tool",
        Some(vec![json!({"id": "call_ok", "name": "t", "input": {}})]),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    ghost::db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "user",
        "",
        None,
        Some(vec![
            json!({"tool_use_id": "call_ok", "content": "result", "is_error": false}),
        ]),
        None,
        None,
    )
    .await
    .unwrap();

    let provider = Arc::new(MockProvider::new(vec![respond_response("ok", vec![])]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let _ = chat
        .chat(&session_id, "continue", None, None)
        .await
        .expect("chat result");

    // DB should have exactly 5 messages: user, assistant+tool, user+result,
    // user "continue", assistant "ok". No repair message.
    let messages = ghost::db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 5, "no repair message should be inserted");
}

// ---------------------------------------------------------------------------
// Compaction tests
// ---------------------------------------------------------------------------

/// Helper: build a config with a tiny context window to trigger compaction.
fn small_context_config(config: &mut ghost::config::Config) {
    config
        .models
        .aliases
        .get_mut("primary")
        .expect("primary alias")
        .context_window = 1000;
}

/// Helper: pre-fill a session with many messages to exceed the context window.
async fn fill_session(db: &ghost::db::GhostDb, session_id: &str, count: usize) {
    for i in 0..count {
        ghost::db::sessions::create_message(
            db,
            session_id,
            if i % 2 == 0 { "user" } else { "assistant" },
            &format!("Message {i}: {}", "x".repeat(200)),
        )
        .await
        .expect("create filler message");
    }
}

#[tokio::test]
async fn compaction_triggers_when_over_threshold() {
    let (db, mut config, _workspace, _config_dir) = common::test_database().await;
    small_context_config(&mut config);

    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    fill_session(&db, &session_id, 30).await;

    // Mock responses: 1st = compaction summary, 2nd = final answer
    let provider = Arc::new(MockProvider::new(vec![
        response(
            vec![ContentBlock::Text {
                text: "Summary of previous conversation.".to_string(),
            }],
            StopReason::EndTurn,
        ),
        respond_response("post-compaction", vec![]),
    ]));
    let requests = provider.requests();
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));

    let result = chat
        .chat(&session_id, "new question", None, None)
        .await
        .expect("chat result");
    assert_eq!(result.0.message, "post-compaction");

    // Verify the second request (the actual chat) has a summary injected
    let recorded = requests.lock().expect("lock requests");
    assert_eq!(recorded.len(), 2, "expected compaction + chat requests");

    // The second request should contain the summary as a system message
    let chat_request = &recorded[1];
    let has_summary = chat_request.messages.iter().any(|msg| {
        msg.role == ghost::providers::Role::System
            && msg.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("Summary of previous conversation")
                )
            })
    });
    assert!(
        has_summary,
        "expected summary in chat request messages: {:?}",
        chat_request.messages
    );

    // Verify fewer messages than the original 30+ are sent to the provider
    let message_count = chat_request.messages.len();
    assert!(
        message_count < 30,
        "expected fewer than 30 messages after compaction, got {message_count}"
    );
}

#[tokio::test]
async fn original_messages_preserved_after_compaction() {
    let (db, mut config, _workspace, _config_dir) = common::test_database().await;
    small_context_config(&mut config);

    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    fill_session(&db, &session_id, 30).await;

    let provider = Arc::new(MockProvider::new(vec![
        response(
            vec![ContentBlock::Text {
                text: "Summary.".to_string(),
            }],
            StopReason::EndTurn,
        ),
        respond_response("ok", vec![]),
    ]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));

    let _ = chat
        .chat(&session_id, "check", None, None)
        .await
        .expect("chat result");

    // All original messages (30 filler + 1 user "check" + 1 assistant "ok")
    // should still be in the database
    let messages = ghost::db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("list messages");
    assert!(
        messages.len() >= 32,
        "expected at least 32 messages in DB, got {}",
        messages.len()
    );

    // Session should have a compaction summary set
    let session = ghost::db::sessions::get_session(&db, &session_id)
        .await
        .expect("get session");
    assert!(
        session.compaction_summary.is_some(),
        "expected compaction_summary to be set"
    );
    assert!(
        session.compaction_cursor_id.is_some(),
        "expected compaction_cursor_id to be set"
    );
}

#[tokio::test]
async fn no_compaction_below_threshold() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    // Default context_window = 200000, so a short conversation won't trigger

    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![respond_response(
        "no compact",
        vec![],
    )]));
    let requests = provider.requests();
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));

    let result = chat
        .chat(&session_id, "short message", None, None)
        .await
        .expect("chat result");
    assert_eq!(result.0.message, "no compact");

    // Only 1 request should have been made (no compaction LLM call)
    {
        let recorded = requests.lock().expect("lock requests");
        assert_eq!(
            recorded.len(),
            1,
            "expected exactly 1 request (no compaction)"
        );
    }

    // Session should have no compaction summary
    let session = ghost::db::sessions::get_session(&db, &session_id)
        .await
        .expect("get session");
    assert!(session.compaction_summary.is_none());
    assert!(session.compaction_cursor_id.is_none());
}

#[tokio::test]
async fn double_compaction_summary_of_summary() {
    let (db, mut config, _workspace, _config_dir) = common::test_database().await;
    small_context_config(&mut config);

    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    // Round 1: fill + compact
    fill_session(&db, &session_id, 30).await;

    let provider = Arc::new(MockProvider::new(vec![
        // Compaction summary for round 1
        response(
            vec![ContentBlock::Text {
                text: "First summary.".to_string(),
            }],
            StopReason::EndTurn,
        ),
        // Chat response for round 1
        respond_response("round1", vec![]),
    ]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), shared(&config));
    let r1 = chat
        .chat(&session_id, "round1 question", None, None)
        .await
        .expect("round 1");
    assert_eq!(r1.0.message, "round1");

    // Round 2: fill more + compact again (summary of summary)
    fill_session(&db, &session_id, 30).await;

    let provider2 = Arc::new(MockProvider::new(vec![
        // Compaction summary for round 2 (summarizes "First summary" + new msgs)
        response(
            vec![ContentBlock::Text {
                text: "Second summary, incorporating first.".to_string(),
            }],
            StopReason::EndTurn,
        ),
        // Chat response for round 2
        respond_response("round2", vec![]),
    ]));
    let requests2 = provider2.requests();
    let chat2 = SessionChat::new(db.clone(), provider2, ToolManager::empty(), shared(&config));

    let r2 = chat2
        .chat(&session_id, "round2 question", None, None)
        .await
        .expect("round 2");
    assert_eq!(r2.0.message, "round2");

    // The second compaction summary should be stored
    let session = ghost::db::sessions::get_session(&db, &session_id)
        .await
        .expect("get session");
    assert_eq!(
        session.compaction_summary.as_deref(),
        Some("Second summary, incorporating first.")
    );

    // The chat request in round 2 should use the second summary
    let recorded = requests2.lock().expect("lock requests");
    let chat_request = &recorded[1];
    let has_second_summary = chat_request.messages.iter().any(|msg| {
        msg.role == ghost::providers::Role::System
            && msg.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("Second summary")
                )
            })
    });
    assert!(
        has_second_summary,
        "expected second summary in round 2 messages"
    );
}
