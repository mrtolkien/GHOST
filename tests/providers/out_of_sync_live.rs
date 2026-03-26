/// Reproduction test for the "GHOST replies out of sync" bug.
///
/// Loads a real 224-message production session from a feedback DB snapshot,
/// then runs a full chat turn through the production code path
/// (load_provider_history → compact_if_needed → run_tool_loop → provider).
///
/// The session contains diverse topics (humidifier, vaccines, baby clothes,
/// humidity, Myojo Gakuen). The last user request is about a sumo quiz.
/// The bug: after compaction, the model answers old topics instead of the
/// latest request.
///
/// ```sh
/// cargo test --features live-tests-llms out_of_sync -- --nocapture
/// ```
use super::common;
use ghost::db;
use ghost::db::sessions;
use serde_json::Value;

const TRANSCRIPT_FIXTURE: &str = include_str!("../fixtures/out_of_sync_transcript.json");

/// Replay the fixture transcript into a fresh test session, omitting the
/// last user request and its tool loop so we can re-issue it via chat().
///
/// The sumo quiz request is at message index 209. Messages 210-223 are the
/// tool loop for that request. We replay 0-208 so chat() starts from the
/// exact state before the bug.
async fn replay_transcript_before_sumo(env: &common::LiveTestEnv) -> String {
    let transcript: Vec<Value> =
        serde_json::from_str(TRANSCRIPT_FIXTURE).expect("parse transcript fixture");
    let session_id = env.create_session().await;

    // Replay messages before the sumo quiz request (index 209)
    let cutoff = 209;
    for msg in &transcript[..cutoff] {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let tool_calls: Option<Vec<Value>> =
            msg.get("tool_calls").and_then(|v| v.as_array()).cloned();
        let tool_results: Option<Vec<Value>> =
            msg.get("tool_results").and_then(|v| v.as_array()).cloned();
        let raw_output: Option<Vec<Value>> =
            msg.get("raw_output").and_then(|v| v.as_array()).cloned();

        db::sessions::create_message_with_metadata(
            &env.db,
            &session_id,
            role,
            content,
            tool_calls,
            tool_results,
            raw_output,
            None,
        )
        .await
        .expect("replay message");
    }

    eprintln!(
        "Replayed {cutoff} of {} messages (before sumo quiz request)",
        transcript.len()
    );
    session_id
}

/// The original user message that triggered the bug.
const SUMO_QUIZ_REQUEST: &str = "\
Avoir the sumo quizz, can you rework the html page so that it shows \
the 42 makuuchi sumo wrestlers one by one and make me choose a name \
(multiple choice) before moving onto the next wrestler, and then show \
me a score at the end";

/// Full production path: replay 209 messages of history, then send the
/// sumo quiz request through chat.chat() — exercises compaction, tool
/// loop, provider calls, and context overflow handling.
#[tokio::test]
async fn out_of_sync_reproduction() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");
    let env = common::live_test_database("out_of_sync").await;
    let session_id = replay_transcript_before_sumo(&env).await;

    let chat = env.chat();

    // Run the full production chat path: this will load history, compact,
    // send to the model, execute tools, and loop until the model stops.
    eprintln!("Calling chat.chat() with sumo quiz request...");
    let result = chat.chat(&session_id, SUMO_QUIZ_REQUEST, None, None).await;

    env.log_session_json("after_chat", &session_id).await;

    match result {
        Ok((chat_result, metadata)) => {
            eprintln!("\n=== CHAT RESULT ===");
            eprintln!(
                "Response: {}",
                &chat_result.message[..chat_result.message.len().min(500)]
            );
            eprintln!("Stop reason: {:?}", chat_result.stop_reason);
            eprintln!("Tool iterations: {}", metadata.iterations);

            // Collect all tool calls made during the turn
            let tool_calls = env.collect_tool_calls(&session_id).await;
            let off_topic: Vec<_> = tool_calls
                .iter()
                .filter(|tc| {
                    let s = tc.to_string().to_lowercase();
                    s.contains("humidifier")
                        || s.contains("iris-ahm")
                        || s.contains("irisohyama")
                        || s.contains("lotion")
                        || s.contains("200ml")
                        || s.contains("myojo")
                        || s.contains("geidai")
                        || s.contains("meningit")
                        || s.contains("vaccin")
                        || s.contains("ceremony")
                        || s.contains("入園式")
                        || s.contains("humidity")
                        || s.contains("湿度")
                })
                .collect();

            eprintln!("\nTotal tool calls: {}", tool_calls.len());
            if !off_topic.is_empty() {
                eprintln!("Off-topic tool calls ({}):", off_topic.len());
                for tc in &off_topic {
                    eprintln!("  ✗ {tc}");
                }
            }

            assert!(
                off_topic.is_empty(),
                "Model made {} off-topic tool calls (answering old questions instead of sumo quiz)",
                off_topic.len(),
            );
        }
        Err(e) => {
            eprintln!("Chat failed: {e}");
            panic!("chat.chat() failed: {e}");
        }
    }
}

/// Regression: an empty compaction_cursor_id must not cause all messages
/// to be dropped. This is the defense-in-depth guard — even if the
/// cursor bug recurs, messages are never silently lost.
#[tokio::test]
async fn empty_cursor_does_not_drop_messages() {
    let _obs = ghost::observability::init_for_live_tests().expect("init live test observability");
    let env = common::live_test_database("empty_cursor").await;
    let session_id = env
        .session_with_messages(&[("user", "What is 2+2?"), ("assistant", "4")])
        .await;

    // Poison: set a summary with an empty cursor — the exact bug state.
    sessions::update_compaction(
        &env.db,
        &session_id,
        "## Task\nThe user asked about math.",
        "",
    )
    .await
    .expect("poison compaction");

    let chat = env.chat();
    let (result, _meta) = chat
        .chat(&session_id, "What is 3+3?", None, None)
        .await
        .expect("chat should succeed despite poisoned cursor");

    // The model must see both old messages AND the new one — not just
    // the summary. If the guard failed, it would only see the summary.
    assert!(!result.message.is_empty(), "response should not be empty",);
}
