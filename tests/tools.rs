mod common;

use ghost::db;
use ghost::tools::{TodoStatus, ToolContext, ToolManager};
use serde_json::json;

fn tool_ctx(
    config: &ghost::config::Config,
    db: &ghost::db::GhostDb,
    session_id: &str,
) -> ToolContext {
    ToolContext {
        workspace: config.workspace.clone(),
        cwd: config.workspace.clone(),
        db: db.clone(),
        config: std::sync::Arc::new(config.clone()),
        session_id: session_id.to_string(),
        agent_runner: None,
        event_tx: None,
        channel_id: None,
        confirmation_tx: None,
        browser_manager: std::sync::Arc::new(tokio::sync::Mutex::new(
            ghost::web::browser::BrowserManager::new(vec![]),
        )),
    }
}

#[tokio::test]
async fn for_chat_registers_expected_tools() {
    let manager = ToolManager::for_chat();
    let schemas = manager.all_tool_schemas();
    assert_eq!(schemas.len(), 8, "expected 8 tools, got {}", schemas.len());

    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"shell"));
    assert!(names.contains(&"file_read"));
    assert!(names.contains(&"file_write"));
    assert!(names.contains(&"file_edit"));
    assert!(names.contains(&"knowledge_search"));
    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"web_fetch"));
    assert!(names.contains(&"agent"));
    assert!(!names.contains(&"todo"), "todo should not be in chat tools");

    for schema in &schemas {
        assert!(!schema.description.is_empty());
        assert!(schema.input_schema.is_object());
    }
}

#[tokio::test]
async fn for_agent_includes_knowledge_tools() {
    let tools: Vec<String> = vec![
        "shell",
        "file_read",
        "file_write",
        "file_edit",
        "todo",
        "knowledge_search",
        "web_search",
        "web_fetch",
        "note_write",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let agent_manager = ToolManager::for_agent(&tools);
    assert_eq!(agent_manager.all_tool_schemas().len(), 9);

    let schemas = agent_manager.all_tool_schemas();
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"note_write"));
    // agent should NOT be included
    assert!(!names.contains(&"agent"));
}

#[tokio::test]
async fn todo_round_trip_through_db() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let ctx = tool_ctx(&config, &db, &session_id);
    let manager = ToolManager::for_agent(&["todo".to_string()]);

    // Plan
    let result = manager
        .execute(
            "todo",
            json!({
                "action": "plan",
                "items": [
                    {"title": "Research the problem"},
                    {"title": "Implement solution", "description": "Write the code"},
                    {"title": "Test and verify"}
                ]
            }),
            &ctx,
        )
        .await
        .expect("todo plan");

    assert!(result.text.contains("TODO [0/3]"));
    assert!(result.text.contains("Research the problem"));

    // Update one
    let result = manager
        .execute(
            "todo",
            json!({
                "action": "update",
                "index": 1,
                "status": "done",
                "note": "found the root cause"
            }),
            &ctx,
        )
        .await
        .expect("todo update");

    assert!(result.text.contains("TODO [1/3]"));
    assert!(result.text.contains("found the root cause"));

    // Verify persistence
    let items = db::sessions::get_session_todo_list(&db, &session_id)
        .await
        .expect("get todo list")
        .expect("todo list exists");

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].status, TodoStatus::Done);
    assert_eq!(items[0].note.as_deref(), Some("found the root cause"));
    assert_eq!(items[1].status, TodoStatus::Pending);

    // Batch update
    let result = manager
        .execute(
            "todo",
            json!({
                "action": "batch_update",
                "updates": [
                    {"index": 2, "status": "in_progress"},
                    {"index": 3, "status": "skipped", "note": "not needed"}
                ]
            }),
            &ctx,
        )
        .await
        .expect("todo batch_update");

    assert!(result.text.contains("TODO [2/3]"));

    // Add
    let result = manager
        .execute(
            "todo",
            json!({
                "action": "add",
                "title": "Deploy"
            }),
            &ctx,
        )
        .await
        .expect("todo add");

    assert!(result.text.contains("TODO [2/4]"));
    assert!(result.text.contains("Deploy"));

    // Clear
    let result = manager
        .execute("todo", json!({"action": "clear"}), &ctx)
        .await
        .expect("todo clear");

    assert!(result.text.contains("cleared"));

    let cleared = db::sessions::get_session_todo_list(&db, &session_id)
        .await
        .expect("get cleared list");
    assert!(cleared.is_none());
}

#[tokio::test]
async fn chained_write_edit_read() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let ctx = tool_ctx(&config, &db, &session_id);
    let manager = ToolManager::for_chat();

    // Write a file
    let result = manager
        .execute(
            "file_write",
            json!({
                "path": "test.txt",
                "content": "line 1\nline 2\nline 3\n"
            }),
            &ctx,
        )
        .await
        .expect("file_write");

    assert!(result.text.contains("Created"));

    // Edit the file
    let result = manager
        .execute(
            "file_edit",
            json!({
                "path": "test.txt",
                "old_string": "line 2",
                "new_string": "line two (edited)"
            }),
            &ctx,
        )
        .await
        .expect("file_edit");

    assert!(result.text.contains("Edited"));

    // Read the file
    let result = manager
        .execute("file_read", json!({"path": "test.txt"}), &ctx)
        .await
        .expect("file_read");

    assert!(result.text.contains("line two (edited)"));
    assert!(result.text.contains("1 | line 1"));
    assert!(result.text.contains("3 | line 3"));
}

#[tokio::test]
async fn shell_runs_in_workspace() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let ctx = tool_ctx(&config, &db, &session_id);
    let manager = ToolManager::for_chat();

    let result = manager
        .execute("shell", json!({"command": "pwd"}), &ctx)
        .await
        .expect("shell pwd");

    assert!(result.text.contains("Exit code: 0"));
}

#[tokio::test]
async fn todo_invalid_index_returns_error() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let ctx = tool_ctx(&config, &db, &session_id);
    let manager = ToolManager::for_agent(&["todo".to_string()]);

    // Plan with one item
    manager
        .execute(
            "todo",
            json!({
                "action": "plan",
                "items": [{"title": "Only item"}]
            }),
            &ctx,
        )
        .await
        .expect("plan");

    // Try updating index 5 (out of range)
    let result = manager
        .execute(
            "todo",
            json!({
                "action": "update",
                "index": 5,
                "status": "done"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn unknown_tool_returns_not_found() {
    let manager = ToolManager::for_chat();
    let ctx = ToolContext {
        workspace: std::path::PathBuf::from("/tmp"),
        cwd: std::path::PathBuf::from("/tmp"),
        db: sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
        config: std::sync::Arc::new(ghost::config::test_config(std::path::Path::new("/tmp"))),
        session_id: "test".to_string(),
        agent_runner: None,
        event_tx: None,
        channel_id: None,
        confirmation_tx: None,
        browser_manager: std::sync::Arc::new(tokio::sync::Mutex::new(
            ghost::web::browser::BrowserManager::new(vec![]),
        )),
    };

    let result = manager.execute("nonexistent_tool", json!({}), &ctx).await;

    assert!(result.is_err());
}
