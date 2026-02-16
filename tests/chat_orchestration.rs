mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ghost::chat::{ChatStopReason, SessionChat};
use ghost::providers::{
    ChatRequest, ChatResponse, ContentBlock, Provider, ProviderError, StopReason, ToolDefinition,
};
use ghost::tools::{Tool, ToolContext, ToolError, ToolManager};
use serde_json::json;

#[derive(Debug)]
struct MockProvider {
    responses: Arc<Mutex<VecDeque<ChatResponse>>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl MockProvider {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Arc<Mutex<Vec<ChatRequest>>> {
        Arc::clone(&self.requests)
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
        self.requests.lock().expect("lock requests").push(request);
        self.responses
            .lock()
            .expect("lock responses")
            .pop_front()
            .ok_or_else(|| ProviderError::InvalidResponse("no mock response remaining".to_string()))
    }

    fn name(&self) -> &str {
        "mock"
    }
}

#[derive(Debug)]
struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_tool"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo_tool".to_string(),
            description: "echoes input".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let text = params
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(format!("echo:{text}"))
    }
}

fn response(content: Vec<ContentBlock>, stop_reason: StopReason) -> ChatResponse {
    ChatResponse {
        content,
        usage: ghost::providers::Usage::default(),
        stop_reason,
        model: "mock-model".to_string(),
    }
}

#[tokio::test]
async fn chat_returns_response_text() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![response(
        vec![ContentBlock::Text(
            r#"{"message":"hello from ghost","citations":[]}"#.to_string(),
        )],
        StopReason::EndTurn,
    )]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::new(), config);
    let result = chat
        .chat(&session_id.to_string(), "hi")
        .await
        .expect("chat result");

    assert_eq!(result.message, "hello from ghost");
    assert_eq!(result.stop_reason, ChatStopReason::EndTurn);
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
        response(
            vec![ContentBlock::Text(
                r#"{"message":"done","citations":[]}"#.to_string(),
            )],
            StopReason::EndTurn,
        ),
    ]));
    let requests = provider.requests();

    let mut tools = ToolManager::new();
    tools.register(Arc::new(EchoTool));
    let chat = SessionChat::new(db.clone(), provider, tools, config);

    let result = chat
        .chat(&session_id.to_string(), "run tool")
        .await
        .expect("chat result");
    assert_eq!(result.message, "done");

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

    let mut tools = ToolManager::new();
    tools.register(Arc::new(EchoTool));
    let chat = SessionChat::new(db.clone(), provider, tools, config).with_max_tool_iterations(1);
    let result = chat
        .chat(&session_id.to_string(), "loop")
        .await
        .expect("chat result");
    assert_eq!(result.stop_reason, ChatStopReason::MaxIterations);
}

#[tokio::test]
async fn chat_persists_user_and_assistant_messages() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let provider = Arc::new(MockProvider::new(vec![response(
        vec![ContentBlock::Text(
            r#"{"message":"persisted","citations":[]}"#.to_string(),
        )],
        StopReason::EndTurn,
    )]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::new(), config);

    let _ = chat
        .chat(&session_id.to_string(), "persist")
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
    let chat = SessionChat::new(db.clone(), provider, ToolManager::new(), config);
    let new_session = chat
        .reboot_session(&old_session_id.to_string())
        .await
        .expect("reboot session");

    assert_ne!(new_session, old_session_id.to_string());

    let old_record = ghost::db::sessions::get_session(&db, &old_session_id)
        .await
        .expect("old session");
    assert_eq!(old_record.status, "rebooted");

    let active =
        ghost::db::interface_sessions::get_active_session_for_interface(&db, "discord:channel:123")
            .await
            .expect("active")
            .expect("active session");
    assert_eq!(active.to_string(), new_session);
}

#[tokio::test]
async fn structured_output_populates_citations_and_creates_edges() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");

    let _reference_id = ghost::db::knowledge::create_reference(
        &db,
        "db",
        "knowledge/references/surrealdb/relate.md",
        "relate docs",
        Some("https://docs.surrealdb.com"),
    )
    .await
    .expect("create reference");

    let provider = Arc::new(MockProvider::new(vec![response(
        vec![ContentBlock::Text(
            r#"{"message":"SurrealDB uses RELATE.","citations":[{"source":"knowledge/references/surrealdb/relate.md","context":"relate docs"}]}"#.to_string(),
        )],
        StopReason::EndTurn,
    )]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::new(), config);

    let result = chat
        .chat(&session_id.to_string(), "How do edges work?")
        .await
        .expect("chat result");
    assert_eq!(result.citations.len(), 1);
    assert_eq!(
        result.citations[0].source,
        "knowledge/references/surrealdb/relate.md"
    );

    let assistant = ghost::db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("messages")
        .into_iter()
        .find(|message| message.role == "assistant")
        .expect("assistant message");
    assert!(assistant.citations.is_some());

    #[derive(Debug, serde::Deserialize)]
    struct EdgeRow {
        edge_count: i64,
    }
    let mut edge_resp = db
        .query("SELECT count() AS edge_count FROM cited WHERE in = $message_id GROUP ALL")
        .bind(("message_id", assistant.id))
        .await
        .expect("query edges");
    let edge_rows: Vec<EdgeRow> = edge_resp.take(0).expect("take rows");
    assert_eq!(edge_rows.first().map(|row| row.edge_count), Some(1));
}

#[tokio::test]
async fn web_cache_citation_resolves_url_from_frontmatter() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    let cache_path = config.workspace.join(".web-cache/example.md");
    std::fs::create_dir_all(cache_path.parent().expect("cache parent")).expect("create dir");
    std::fs::write(
        &cache_path,
        "---\nurl: https://example.com/article\n---\nBody",
    )
    .expect("write cache file");

    let provider = Arc::new(MockProvider::new(vec![response(
        vec![ContentBlock::Text(
            r#"{"message":"cached","citations":[{"source":".web-cache/example.md"}]}"#.to_string(),
        )],
        StopReason::EndTurn,
    )]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::new(), config);
    let result = chat
        .chat(&session_id.to_string(), "cite web cache")
        .await
        .expect("chat result");

    assert_eq!(
        result.citations[0].url.as_deref(),
        Some("https://example.com/article")
    );
}

#[tokio::test]
async fn todo_state_is_injected_after_user_message() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    db.query("UPDATE $session_id SET todo_list = ['Step 1']")
        .bind(("session_id", session_id.clone()))
        .await
        .expect("set todo");

    let provider = Arc::new(MockProvider::new(vec![response(
        vec![ContentBlock::Text(
            r#"{"message":"ok","citations":[]}"#.to_string(),
        )],
        StopReason::EndTurn,
    )]));
    let requests = provider.requests();
    let chat = SessionChat::new(db.clone(), provider, ToolManager::new(), config);
    let _ = chat
        .chat(&session_id.to_string(), "check todo")
        .await
        .expect("chat result");

    let requests = requests.lock().expect("lock requests");
    let first = requests.first().expect("request");
    assert!(
        first
            .system
            .as_deref()
            .unwrap_or_default()
            .contains("GHOST")
    );
    let todo_message_index = first.messages.iter().position(|message| {
        message.role == ghost::providers::Role::System
            && message.content.iter().any(
                |block| matches!(block, ContentBlock::Text(text) if text.contains("Current TODO")),
            )
    });
    assert!(
        todo_message_index.is_some(),
        "messages sent to provider: {:?}",
        first.messages
    );
    let todo_message_index = todo_message_index.expect("todo injection");
    let user_message_index = first
        .messages
        .iter()
        .position(|message| {
            message.role == ghost::providers::Role::User
                && message
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::Text(text) if text == "check todo"))
        })
        .expect("user message");
    assert!(todo_message_index > user_message_index);
}
