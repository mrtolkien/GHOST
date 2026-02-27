mod common;

use ghost::db;

#[tokio::test]
async fn schema_apply_is_idempotent() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;

    // Drop the first connection before reconnecting to verify idempotent
    // schema application.
    drop(db);
    // Give the async runtime a moment to release the lock file.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let reconnect = db::connect(&config.workspace, config.embeddings.dimension).await;
    assert!(reconnect.is_ok(), "reconnect failed: {:?}", reconnect.err());
}

#[tokio::test]
async fn session_message_note_and_edges_work() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let session = db::sessions::get_session(&db, &session_id)
        .await
        .expect("get session");
    assert_eq!(session.id, session_id);

    db::sessions::update_activity(&db, &session_id)
        .await
        .expect("update activity");

    let _message_id = db::sessions::create_message(&db, &session_id, "user", "hello")
        .await
        .expect("create message");
    let messages = db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("list messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "hello");

    let rust_note_id = db::knowledge::create_note(&db, "Rust", "systems language")
        .await
        .expect("create source note");
    let ghost_note_id = db::knowledge::create_note(&db, "GHOST", "agent project")
        .await
        .expect("create target note");

    let _edge_id = db::knowledge::create_edge(&db, &ghost_note_id, &rust_note_id, "written_in")
        .await
        .expect("create edge");

    let related = db::knowledge::related_note_ids(&db, &ghost_note_id)
        .await
        .expect("query related notes");
    assert_eq!(related, vec![rust_note_id]);

    let rust_note = db::knowledge::get_note(&db, &related[0])
        .await
        .expect("get related note");
    assert_eq!(rust_note.title, "Rust");
}

#[tokio::test]
async fn message_tool_calls_round_trip() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");

    // Store assistant message with tool_calls
    let tool_calls = vec![serde_json::json!({
        "id": "call_1",
        "name": "web_search",
        "input": {"query": "best 3d printers 2026"}
    })];
    db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "assistant",
        "",
        Some(tool_calls.clone()),
        None,
        None,
    )
    .await
    .expect("create assistant message with tool_calls");

    // Store user message with tool_results
    let tool_results = vec![serde_json::json!({
        "tool_use_id": "call_1",
        "content": "Found 10 results...",
        "is_error": false
    })];
    db::sessions::create_message_with_metadata(
        &db,
        &session_id,
        "user",
        "",
        None,
        Some(tool_results.clone()),
        None,
    )
    .await
    .expect("create user message with tool_results");

    // Read back
    let messages = db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("list messages");
    assert_eq!(messages.len(), 2);

    // Verify assistant tool_calls
    let assistant = &messages[0];
    assert_eq!(assistant.role, "assistant");
    let calls = assistant
        .tool_calls_parsed()
        .expect("tool_calls should be Some");
    assert_eq!(calls.len(), 1, "expected 1 tool call, got: {calls:?}");
    assert_eq!(
        calls[0].get("name").and_then(|v| v.as_str()),
        Some("web_search")
    );

    // Verify user tool_results
    let user = &messages[1];
    assert_eq!(user.role, "user");
    let results = user
        .tool_results_parsed()
        .expect("tool_results should be Some");
    assert_eq!(results.len(), 1, "expected 1 tool result, got: {results:?}");
    assert_eq!(
        results[0].get("tool_use_id").and_then(|v| v.as_str()),
        Some("call_1")
    );
}
