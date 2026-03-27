/// Live tests validating that the Anthropic message conversion pipeline
/// correctly handles message adjacency constraints.
///
/// Anthropic requires:
/// - `tool_result` must be in the message immediately after `tool_use`
/// - The user message after tool_use must contain ONLY tool_result blocks
/// - No consecutive same-role messages
///
/// These tests seed a session DB with problematic message patterns, then
/// run a full chat turn through SessionChat (DB → load_provider_history →
/// convert_messages → Anthropic API). A 200 response proves the structure
/// is valid.
///
/// IMPORTANT: These tests MUST hit the Anthropic API directly (not
/// OpenRouter or Codex) because only the native Anthropic endpoint
/// enforces strict tool_use/tool_result adjacency constraints.
///
/// ```sh
/// cargo test --features live-tests-llms message_adjacency -- --nocapture
/// ```
use super::common;
use ghost::db;
use ghost::db::sessions::MessagePayload;
use serde_json::json;

/// Model alias that maps to `provider = "anthropic"` in the local config.
/// The test config must have a `[models.test]` entry using the anthropic
/// provider — not openrouter or codex.
const ANTHROPIC_MODEL: &str = "test";

type MessageTuple<'a> = (
    &'a str,
    &'a str,
    Option<Vec<serde_json::Value>>,
    Option<Vec<serde_json::Value>>,
);

/// Helper: seed a session with messages, then chat. Returns the response text
/// or panics with the Anthropic error.
async fn seed_and_chat(
    test_name: &str,
    messages: &[MessageTuple<'_>],
    user_prompt: &str,
) -> String {
    // Force the Anthropic provider regardless of the config default.
    // SAFETY: called during single-threaded test setup before any async work
    unsafe { std::env::set_var("GHOST_E2E_MODEL", ANTHROPIC_MODEL) };
    let env = common::live_test_database(test_name).await;
    let session_id = env.create_session().await;

    for (role, content, tool_calls, tool_results) in messages {
        db::sessions::create_message_with_metadata(
            &env.db,
            &session_id,
            role,
            content,
            &MessagePayload {
                tool_calls: tool_calls.clone(),
                tool_results: tool_results.clone(),
                ..Default::default()
            },
        )
        .await
        .expect("create message");
    }

    let chat = env.chat();
    let (result, _meta) = chat
        .chat(&session_id, user_prompt, None, None)
        .await
        .expect("chat should succeed — message structure must be valid");

    env.log_session_json("after_chat", &session_id).await;
    result.message
}

/// System message between assistant tool_use and user tool_result.
///
/// This is the exact pattern from `ghost send-image`: the shell tool runs
/// `send-image`, which injects a system message notification before the
/// tool_result is recorded.
///
/// ```
/// assistant: tool_use(call_X)
/// system:    "[sent image: photo.png]"
/// user:      tool_result(call_X)
/// ```
#[tokio::test]
async fn system_message_between_tool_use_and_result() {
    let response = seed_and_chat(
        "adj_sys_between_tool",
        &[
            ("user", "Send me the screenshot", None, None),
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_test_1",
                    "name": "shell",
                    "input": {"command": "ghost send-image /tmp/photo.png"}
                })]),
                None,
            ),
            // System message injected by send-image
            ("system", "[sent image: photo.png — test image]", None, None),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_test_1",
                    "content": "Exit code: 0\nImage sent: photo.png",
                    "is_error": false
                })]),
            ),
            ("assistant", "Sent the image.", None, None),
        ],
        "Say OK",
    )
    .await;

    assert!(!response.trim().is_empty(), "response should not be empty");
}

/// Two system messages between tool_use and tool_result.
///
/// Edge case: multiple system notifications are injected (e.g. image sent
/// + agent completion) before the tool result.
#[tokio::test]
async fn multiple_system_messages_between_tool_use_and_result() {
    let response = seed_and_chat(
        "adj_multi_sys_between",
        &[
            ("user", "Do the thing", None, None),
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_test_2",
                    "name": "shell",
                    "input": {"command": "ghost send-image /tmp/a.png"}
                })]),
                None,
            ),
            ("system", "[sent image: a.png]", None, None),
            ("system", "[notification: upload confirmed]", None, None),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_test_2",
                    "content": "Exit code: 0",
                    "is_error": false
                })]),
            ),
            ("assistant", "Done.", None, None),
        ],
        "Say OK",
    )
    .await;

    assert!(!response.trim().is_empty());
}

/// Consecutive user messages (system-as-user + text).
///
/// This is the pattern from deep-research completion: a system message
/// (agent result) followed by a user text message, with no assistant
/// in between.
///
/// ```
/// assistant: "Started research."
/// system:    "[agent:deep-research completed] ..."
/// user:      "[system] Background task completed."
/// ```
#[tokio::test]
async fn consecutive_user_from_system_and_text() {
    let response = seed_and_chat(
        "adj_consec_user",
        &[
            ("user", "Research standing desks in Japan", None, None),
            ("assistant", "Started deep research on this.", None, None),
            (
                "system",
                "[agent:deep-research completed]\n{\"report\": \"KOKUYO is the best\"}",
                None,
                None,
            ),
            ("user", "[system] Background task completed.", None, None),
            (
                "assistant",
                "Based on my research, KOKUYO SEQUENCE is the best standing desk in Japan.",
                None,
                None,
            ),
        ],
        "Say OK",
    )
    .await;

    assert!(!response.trim().is_empty());
}

/// Tool_use → system → tool_result repeated twice in the same session.
///
/// Reproduces the exact production bug: two `send-image` calls in the
/// same conversation, each injecting a system message.
#[tokio::test]
async fn two_send_image_calls_in_same_session() {
    let response = seed_and_chat(
        "adj_two_send_images",
        &[
            ("user", "Show me the reservation QR code", None, None),
            // First send-image
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_img_1",
                    "name": "shell",
                    "input": {"command": "ghost send-image /tmp/qr1.png"}
                })]),
                None,
            ),
            ("system", "[sent image: qr1.png — QR code]", None, None),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_img_1",
                    "content": "Exit code: 0\nImage sent: qr1.png",
                    "is_error": false
                })]),
            ),
            ("assistant", "Sent the QR code.", None, None),
            (
                "user",
                "Can't see it fully, try again with a larger viewport",
                None,
                None,
            ),
            // Browser interactions
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_browser_1",
                    "name": "browser",
                    "input": {"action": "resize", "width": 800, "height": 1200}
                })]),
                None,
            ),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_browser_1",
                    "content": "{\"ok\": true}",
                    "is_error": false
                })]),
            ),
            // Second send-image
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_img_2",
                    "name": "shell",
                    "input": {"command": "ghost send-image /tmp/qr2.png"}
                })]),
                None,
            ),
            (
                "system",
                "[sent image: qr2.png — QR code reframed]",
                None,
                None,
            ),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_img_2",
                    "content": "Exit code: 0\nImage sent: qr2.png",
                    "is_error": false
                })]),
            ),
            ("assistant", "Resent with a larger capture.", None, None),
        ],
        "Say OK",
    )
    .await;

    assert!(!response.trim().is_empty());
}

/// System message that is NOT between tool_use/result — should be
/// emitted normally as a user message, not deferred.
///
/// ```
/// assistant: text (no tool_use)
/// system:    "[agent completed]"
/// user:      "next question"
/// ```
#[tokio::test]
async fn system_message_not_between_tool_pair() {
    let response = seed_and_chat(
        "adj_sys_not_tool",
        &[
            ("user", "Start a background task", None, None),
            ("assistant", "Started the task.", None, None),
            (
                "system",
                "[agent:task-runner completed] Success.",
                None,
                None,
            ),
            ("user", "[system] Background task completed.", None, None),
            ("assistant", "The task finished successfully.", None, None),
        ],
        "Say OK",
    )
    .await;

    assert!(!response.trim().is_empty());
}

/// Full production scenario: session with tool loops, system messages,
/// and agent completions mixed together.
///
/// This is a simplified version of the actual failing session from
/// production, covering:
/// - Normal tool_use/result pairs
/// - send-image injecting system between tool_use/result (×2)
/// - Agent completion as system message between text messages
/// - Regular text exchanges
#[tokio::test]
async fn full_mixed_session() {
    let response = seed_and_chat(
        "adj_full_mixed",
        &[
            ("user", "Book Hama sushi", None, None),
            // Tool loop: file_read
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_fr1",
                    "name": "file_read",
                    "input": {"path": "skills/hama-sushi/skill.md"}
                })]),
                None,
            ),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_fr1",
                    "content": "File content: instructions for booking...",
                    "is_error": false
                })]),
            ),
            // Tool loop: browser
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_br1",
                    "name": "browser",
                    "input": {"action": "navigate", "url": "https://hamazushi.com"}
                })]),
                None,
            ),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_br1",
                    "content": "Page loaded",
                    "is_error": false
                })]),
            ),
            ("assistant", "Booked. Reservation #323.", None, None),
            // User asks for QR — send-image pattern
            ("user", "Show me the QR code", None, None),
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_sh1",
                    "name": "shell",
                    "input": {"command": "ghost send-image /tmp/qr.png"}
                })]),
                None,
            ),
            (
                "system",
                "[sent image: qr.png — reservation QR]",
                None,
                None,
            ),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_sh1",
                    "content": "Exit code: 0\nImage sent",
                    "is_error": false
                })]),
            ),
            ("assistant", "Sent the QR code.", None, None),
            // New topic with deep-research
            ("user", "Research standing desks", None, None),
            (
                "assistant",
                "",
                Some(vec![json!({
                    "id": "call_ag1",
                    "name": "agent",
                    "input": {"archetype": "deep-research", "query": "best standing desk Japan"}
                })]),
                None,
            ),
            (
                "user",
                "",
                None,
                Some(vec![json!({
                    "tool_use_id": "call_ag1",
                    "content": "Agent 'deep-research' started",
                    "is_error": false
                })]),
            ),
            ("assistant", "Started deep research.", None, None),
            // Agent completion as system message
            (
                "system",
                "[agent:deep-research completed]\n{\"report\": \"KOKUYO SEQUENCE\"}",
                None,
                None,
            ),
            ("user", "[system] Background task completed.", None, None),
            (
                "assistant",
                "KOKUYO SEQUENCE is the best option.",
                None,
                None,
            ),
            // Ping/pong
            ("user", "ping", None, None),
            ("assistant", "pong", None, None),
        ],
        "Say OK",
    )
    .await;

    assert!(!response.trim().is_empty());
}

/// Stale `redacted_thinking` blocks from a previous model (e.g. Codex or
/// a prior Anthropic session) cause the API to reject with "Invalid data
/// in redacted_thinking block". The error must surface as
/// `IncompatibleHistory` with a clear message telling the OPERATOR to
/// switch models or /reboot.
#[tokio::test]
async fn stale_redacted_thinking_surfaces_incompatible_history_error() {
    // SAFETY: called during single-threaded test setup before any async work
    unsafe { std::env::set_var("GHOST_E2E_MODEL", ANTHROPIC_MODEL) };
    let env = common::live_test_database("adj_stale_thinking").await;
    let session_id = env.create_session().await;

    db::sessions::create_message(&env.db, &session_id, "user", "Hello")
        .await
        .unwrap();

    // Assistant with stale redacted_thinking from a different model.
    db::sessions::create_message_with_metadata(
        &env.db,
        &session_id,
        "assistant",
        "Hi there!",
        &MessagePayload {
            raw_output: Some(vec![json!({
                "original_type": "redacted_thinking",
                "opaque_data": "gAAAAABpwVA0_FAKE_STALE_DATA_FROM_PREVIOUS_SESSION_xyzzy"
            })]),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let chat = env.chat();
    let result = chat.chat(&session_id, "Say OK", None, None).await;

    match result {
        Err(ghost::chat::ChatError::Provider(
            ghost::providers::ProviderError::IncompatibleHistory(_),
        )) => {
            // Expected: clear error about incompatible thinking blocks.
        }
        Err(e) => panic!("expected IncompatibleHistory, got: {e}"),
        Ok(_) => panic!("expected error, but chat succeeded"),
    }
}
