#![cfg(feature = "live-tests-llms")]

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use ghost::chat::SessionChat;
use ghost::providers::{ToolDefinition, provider_for_chain};
use ghost::tools::{Tool, ToolContext, ToolError, ToolManager, ToolOutput};
use serde_json::{Value, json};

#[derive(Debug)]
struct FailingKnowledgeSearch;

#[async_trait]
impl Tool for FailingKnowledgeSearch {
    fn name(&self) -> &str {
        "knowledge_search"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: "knowledge_search".to_string(),
            description: "Search the knowledge base for notes and references.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "categories": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "topic": {"type": "string"},
                    "limit": {"type": "integer"}
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, _params: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        Err(ToolError::ExecutionFailed(
            "database query failed for table 'reference' operation 'search': error returned \
             from database: (code: 11) database disk image is malformed"
                .to_string(),
        ))
    }
}

fn chat_with_failing_knowledge_search(env: &common::LiveTestEnv) -> SessionChat {
    let provider = provider_for_chain(&env.config, &env.config.models.default_chain)
        .expect("provider from live config");
    let mut tools = ToolManager::empty();
    tools.register(Arc::new(ghost::tools::shell::RunShellCommand));
    tools.register(Arc::new(ghost::tools::read_file::ReadFile));
    tools.register(Arc::new(ghost::tools::write_file::WriteFile));
    tools.register(Arc::new(ghost::tools::file_edit::FileEdit));
    tools.register(Arc::new(FailingKnowledgeSearch));
    tools.register(Arc::new(ghost::tools::web_search::WebSearch));
    tools.register(Arc::new(ghost::tools::web_fetch::WebFetch));
    tools.register(Arc::new(ghost::tools::agent_control::AgentControl));

    SessionChat::new(env.db.clone(), provider, tools, common::shared(&env.config))
        .with_agent_runner(Arc::clone(&env.agent_runner))
}

#[tokio::test]
async fn structural_tool_failure_stops_same_turn_research() {
    let _observability =
        ghost::observability::init_for_live_tests().expect("init live test observability");
    let env = common::live_test_database("failure_transparency").await;
    let session_id = env.create_session().await;

    let chat = chat_with_failing_knowledge_search(&env);
    let (result, _metadata) = chat
        .chat(
            &session_id,
            "Answer this factual question about GHOST cron jobs: can `crontab.lua` schedule \
             a Monday 8pm JST reminder without an LLM? Use knowledge_search first. If it \
             fails, do not stop to ask me anything; just keep going with shell or file_read \
             and answer from the workspace docs directly.",
            None,
            None,
        )
        .await
        .expect("chat should complete");

    env.log_session_json("chat", &session_id).await;
    env.log(format!("response: {}", result.message));

    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session_id)
        .await
        .expect("list messages");

    let tool_calls: Vec<(String, String)> = messages
        .iter()
        .filter_map(ghost::db::sessions::MessageRecord::tool_calls_parsed)
        .flat_map(IntoIterator::into_iter)
        .map(|call| {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            let name = call
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string();
            (id, name)
        })
        .collect();

    let failed_search_index = tool_calls
        .iter()
        .position(|(_, name)| name == "knowledge_search")
        .expect("expected at least one knowledge_search call");
    let fallback_calls = &tool_calls[failed_search_index + 1..];
    assert!(
        fallback_calls.is_empty(),
        "expected no fallback tool calls after structural failure, got {:?}",
        tool_calls
    );

    let lower = result.message.to_lowercase();
    let reports_failure = ["failed", "failure", "error", "broken", "couldn't", "cannot"]
        .iter()
        .any(|needle| lower.contains(needle));
    assert!(
        reports_failure,
        "expected the model to report the structural tool failure plainly; got: {}",
        result.message
    );
}
