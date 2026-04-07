mod common;

use ghost::db;
use ghost::tools::{TodoStatus, ToolContext, ToolManager};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

async fn spawn_static_http_server(
    content_type: &'static str,
    body: &'static [u8],
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let mut buf = [0_u8; 2048];
            let Ok(read) = socket.read(&mut buf).await else {
                continue;
            };
            if read == 0 {
                continue;
            }
            let request = String::from_utf8_lossy(&buf[..read]);
            let is_head = request.starts_with("HEAD ");
            let response = if is_head {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes()
            } else {
                let mut response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                response.extend_from_slice(body);
                response
            };
            let _ = socket.write_all(&response).await;
        }
    });
    (format!("http://{addr}/document"), handle)
}

async fn spawn_docling_mock_server(
    response_json: serde_json::Value,
    captured_submit_body: Arc<Mutex<Option<String>>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind docling server");
    let addr = listener.local_addr().expect("local addr");
    let body = response_json.to_string();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let mut buf = Vec::new();
            let mut chunk = [0_u8; 4096];
            let Ok(read) = socket.read(&mut chunk).await else {
                continue;
            };
            if read == 0 {
                continue;
            }
            buf.extend_from_slice(&chunk[..read]);
            let request = String::from_utf8_lossy(&buf).to_string();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let response_body = if path == "/v1/convert/source/async" {
                if let Some((_, request_body)) = request.split_once("\r\n\r\n") {
                    *captured_submit_body.lock().expect("lock captured body") =
                        Some(request_body.to_string());
                }
                json!({ "task_id": "task-1" }).to_string()
            } else if path == "/v1/status/poll/task-1?wait=5" {
                json!({ "task_status": "success" }).to_string()
            } else if path == "/v1/result/task-1" {
                body.clone()
            } else {
                json!({ "error": "not found" }).to_string()
            };

            let status_line = if path == "/v1/convert/source/async"
                || path == "/v1/status/poll/task-1?wait=5"
                || path == "/v1/result/task-1"
            {
                "HTTP/1.1 200 OK"
            } else {
                "HTTP/1.1 404 Not Found"
            };
            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    (format!("http://{addr}"), handle)
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

#[tokio::test]
async fn web_fetch_pdf_url_uses_docling_url_source() {
    let (db, mut config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");

    let pdf_bytes = b"%PDF-1.4\n% test fixture\n";
    let (pdf_url, pdf_server) =
        spawn_static_http_server("application/pdf", pdf_bytes.as_slice()).await;

    let captured_submit_body = Arc::new(Mutex::new(None));
    let docling_response = json!({
        "document": {
            "json_content": {
                "body": {
                    "children": [{"$ref": "#/texts/0"}]
                },
                "texts": [{
                    "label": "paragraph",
                    "text": "PDF extraction works from web_fetch.",
                    "prov": [{"page_no": 1, "bbox": null, "charspan": []}],
                    "children": [],
                    "level": null
                }],
                "pictures": [],
                "tables": [],
                "groups": [],
                "pages": {
                    "1": {
                        "size": {"width": 612.0, "height": 792.0},
                        "page_no": 1
                    }
                }
            }
        }
    });
    let (docling_url, docling_server) =
        spawn_docling_mock_server(docling_response, captured_submit_body.clone()).await;
    config.docling.url = Some(docling_url);

    let ctx = tool_ctx(&config, &db, &session_id);
    let manager = ToolManager::for_chat();

    let result = manager
        .execute("web_fetch", json!({ "url": pdf_url }), &ctx)
        .await
        .expect("web_fetch pdf");

    pdf_server.abort();
    docling_server.abort();

    assert!(result.text.contains("PDF extraction works from web_fetch."));

    let submit_body = captured_submit_body
        .lock()
        .expect("lock captured body")
        .clone()
        .expect("docling submit body");
    assert!(submit_body.contains("\"kind\":\"http\""));
    assert!(submit_body.contains("\"url\":\"http://"));
    assert!(submit_body.contains("/document"));
}
