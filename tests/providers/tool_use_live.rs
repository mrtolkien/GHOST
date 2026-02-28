use std::collections::BTreeMap;

use ghost::providers::{
    ChatMessage, ChatRequest, ChatResponse, ContentBlock, Provider, Role, StopReason,
    ToolDefinition, user_message,
};
use ghost::tools::ToolManager;

// ── Shared helpers ──────────────────────────────────────────────────────────

fn tool_schemas() -> Vec<ToolDefinition> {
    ToolManager::for_chat().all_tool_schemas()
}

fn assistant_text(text: &str) -> ChatMessage {
    ChatMessage {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
    }
}

fn system_prompt() -> String {
    "You are GHOST, an AI agent with tool access. \
     When the user asks you to perform an action, you MUST call the \
     appropriate tool using the function calling interface. \
     Never describe what you would do — actually call the tool."
        .to_string()
}

/// Build a request with 5 user + 5 assistant messages of history, followed by
/// a 6th user message that explicitly asks to call `run_shell_command`.
fn tool_use_request(model: &str) -> ChatRequest {
    let messages = vec![
        // Exchange 1
        user_message("Hi! I need help setting up my project workspace."),
        assistant_text(
            "Hello! I have tools for running commands, reading/writing files, \
             and managing tasks. Where would you like to start?",
        ),
        // Exchange 2
        user_message("What tools do you have available?"),
        assistant_text(
            "I have five tools:\n\
             1. run_shell_command — execute shell commands\n\
             2. read_file — read file contents with line numbers\n\
             3. write_file — create or overwrite files\n\
             4. file_edit — make targeted edits to existing files\n\
             5. todo — manage a task list",
        ),
        // Exchange 3
        user_message("Great. This is a Rust project with a standard cargo layout."),
        assistant_text(
            "Got it — standard Rust project with src/, tests/, and Cargo.toml. \
             I can run cargo commands, read source files, or organize tasks.",
        ),
        // Exchange 4
        user_message("I'll want to explore the code first before making changes."),
        assistant_text(
            "Good plan. We can start by listing files and reading key modules. \
             Just tell me where to look.",
        ),
        // Exchange 5
        user_message("Let's start by checking where we are in the filesystem."),
        assistant_text("Sure, I'll check that for you right away."),
        // The tool-triggering request
        user_message(
            "Use the run_shell_command tool to execute `pwd`. \
             Do not respond with text — call the tool.",
        ),
    ];

    ChatRequest {
        model: model.to_string(),
        messages,
        tools: Some(tool_schemas()),
        max_tokens: Some(1024),
        temperature: Some(0.0),
        system: Some(system_prompt()),
        reasoning_effort: None,
        cache_key: "test".to_string(),
        turn_state: None,
        debug_context: None,
    }
}

fn assert_called_run_shell_command(response: &ChatResponse) {
    assert_eq!(
        response.stop_reason,
        StopReason::ToolUse,
        "expected ToolUse stop reason, got {:?}.\nContent: {:#?}",
        response.stop_reason,
        response.content,
    );

    let tool_use = response
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
            _ => None,
        })
        .expect("expected at least one ToolUse content block");

    assert_eq!(
        tool_use.1, "run_shell_command",
        "expected tool name 'run_shell_command', got '{}'",
        tool_use.1,
    );
    assert!(
        tool_use.2.get("command").is_some(),
        "expected 'command' parameter in tool input, got: {}",
        tool_use.2,
    );
    assert!(!tool_use.0.is_empty(), "tool call ID should not be empty");
}

// ── Provider tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn openrouter_kimi25_calls_run_shell_command() {
    let _obs = ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("OPENROUTER_API_KEY not set; skipping.");
        return;
    }

    let provider = ghost::providers::OpenRouterProvider::new(BTreeMap::new())
        .expect("OpenRouterProvider init");
    let request = tool_use_request("moonshotai/kimi-k2.5");

    let response = provider.chat(request).await.expect("provider chat");
    assert_called_run_shell_command(&response);
}

#[tokio::test]
async fn kimi_code_calls_run_shell_command() {
    let _obs = ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("KIMI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("KIMI_API_KEY not set; skipping.");
        return;
    }

    let provider =
        ghost::providers::KimiCodeProvider::new(BTreeMap::new()).expect("KimiCodeProvider init");
    let request = tool_use_request("kimi-k2.5");

    let response = provider.chat(request).await.expect("provider chat");
    assert_called_run_shell_command(&response);
}

#[tokio::test]
async fn openai_oauth_gpt53_calls_run_shell_command() {
    let _obs = ghost::observability::init_for_live_tests().expect("init live test observability");

    if ghost::auth::openai_oauth::auth_status()
        .await
        .expect("read oauth auth status")
        .is_none()
    {
        eprintln!("No OpenAI OAuth token found; run `ghost auth codex` first. Skipping.");
        return;
    }

    let provider = ghost::providers::OpenAiOAuthProvider::new(BTreeMap::new())
        .expect("OpenAiOAuthProvider init");
    let request = tool_use_request("gpt-5.3-codex");

    let response = provider.chat(request).await.expect("provider chat");
    assert_called_run_shell_command(&response);
}
