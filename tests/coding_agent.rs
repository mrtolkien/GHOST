mod common;

use std::sync::Arc;

use ghost::chat::{ChatStopReason, SessionChat};
use ghost::coding;
use ghost::config;
use ghost::db;
use ghost::providers::{ContentBlock, StopReason};
use ghost::tools::ToolManager;
use serde_json::json;
use tempfile::TempDir;

use common::{EchoTool, MockProvider, respond_response, response};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a mock git repo with an initial commit so git commands work.
async fn create_mock_repo() -> TempDir {
    let dir = TempDir::new().expect("create temp repo dir");
    let path = dir.path();

    tokio::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .await
        .expect("git init");

    tokio::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .await
        .expect("git config email");

    tokio::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .await
        .expect("git config name");

    tokio::fs::write(path.join("README.md"), "# Test Repo\n")
        .await
        .expect("write README");

    tokio::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .await
        .expect("git add");

    tokio::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output()
        .await
        .expect("git commit");

    dir
}

// ---------------------------------------------------------------------------
// Session lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coding_session_start_creates_db_records() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    let session = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("chan123".to_string()),
        Some("implement feature X".to_string()),
    )
    .await
    .expect("start coding session");

    assert!(!session.id.is_empty());
    assert!(!session.session_id.is_empty());
    assert_eq!(session.working_dir, repo.path());
    assert_eq!(session.channel_id.as_deref(), Some("chan123"));

    // Verify coding session exists in DB
    let (_, _, status) = db::coding_sessions::get_coding_session(&db, &session.id)
        .await
        .expect("get coding session")
        .expect("coding session should exist");
    assert_eq!(status, "active");

    // Verify takeover is active
    let takeover = db::coding_sessions::get_active_takeover(&db, "chan123")
        .await
        .expect("get takeover");
    assert!(takeover.is_some());
    let (id, sid, dir) = takeover.unwrap();
    assert_eq!(id, session.id);
    assert_eq!(sid, session.session_id);
    assert_eq!(dir, repo.path().display().to_string());

    // Verify initial prompt was stored
    let messages = db::sessions::list_messages_by_session(&db, &session.session_id)
        .await
        .expect("list messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "user");
    assert!(messages[0].content.contains("implement feature X"));
}

#[tokio::test]
async fn coding_session_start_without_prompt_stores_no_messages() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    let session = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        None,
        None,
    )
    .await
    .expect("start coding session");

    let messages = db::sessions::list_messages_by_session(&db, &session.session_id)
        .await
        .expect("list messages");
    assert!(messages.is_empty());
}

#[tokio::test]
async fn coding_session_end_generates_git_summary() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    let session = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("chan-end".to_string()),
        None,
    )
    .await
    .expect("start");

    // Add a commit so the summary has content
    tokio::fs::write(repo.path().join("new_file.rs"), "fn main() {}\n")
        .await
        .expect("write file");
    tokio::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo.path())
        .output()
        .await
        .expect("git add");
    tokio::process::Command::new("git")
        .args(["commit", "-m", "add new_file.rs"])
        .current_dir(repo.path())
        .output()
        .await
        .expect("git commit");

    let summary = coding::session::end(&db, &session.id, repo.path())
        .await
        .expect("end coding session");

    assert!(summary.contains("Branch:"), "summary should contain branch");
    assert!(
        summary.contains("new_file.rs"),
        "summary should mention changed file: {summary}"
    );

    // Verify session is ended
    let (_, _, status) = db::coding_sessions::get_coding_session(&db, &session.id)
        .await
        .expect("get session")
        .expect("session should exist");
    assert_eq!(status, "ended");

    // Takeover should be cleared
    let takeover = db::coding_sessions::get_active_takeover(&db, "chan-end")
        .await
        .expect("get takeover");
    assert!(takeover.is_none());
}

// ---------------------------------------------------------------------------
// Prompt building
// ---------------------------------------------------------------------------

#[test]
fn coding_prompt_includes_working_dir() {
    let ws = TempDir::new().unwrap();
    let config = config::test_config(ws.path());
    let repo = TempDir::new().unwrap();

    let prompt = coding::prompt::build_coding_prompt(&config, repo.path());
    assert!(
        prompt.contains(&repo.path().display().to_string()),
        "prompt should contain working_dir"
    );
}

#[test]
fn coding_prompt_includes_repo_context() {
    let ws = TempDir::new().unwrap();
    let config = config::test_config(ws.path());
    let repo = TempDir::new().unwrap();

    std::fs::write(repo.path().join("CLAUDE.md"), "# My Project\nUse tabs.")
        .expect("write CLAUDE.md");

    let prompt = coding::prompt::build_coding_prompt(&config, repo.path());
    assert!(
        prompt.contains("Use tabs."),
        "prompt should include CLAUDE.md content"
    );
    assert!(
        prompt.contains("CLAUDE.md"),
        "prompt should mention source file"
    );
}

#[test]
fn coding_prompt_prefers_agents_md_over_claude_md() {
    let ws = TempDir::new().unwrap();
    let config = config::test_config(ws.path());
    let repo = TempDir::new().unwrap();

    std::fs::write(repo.path().join("AGENTS.md"), "agents-content").expect("write AGENTS.md");
    std::fs::write(repo.path().join("CLAUDE.md"), "claude-content").expect("write CLAUDE.md");

    let prompt = coding::prompt::build_coding_prompt(&config, repo.path());
    assert!(prompt.contains("agents-content"));
    assert!(!prompt.contains("claude-content"));
}

#[test]
fn coding_prompt_discovers_workspace_skills() {
    let ws = TempDir::new().unwrap();
    ghost::config_workspace::bootstrap_workspace(
        &config::test_config(ws.path()),
    )
    .expect("bootstrap");
    ghost::skills::install_default_skills(ws.path()).expect("install skills");

    let config = config::test_config(ws.path());
    let repo = TempDir::new().unwrap();

    let prompt = coding::prompt::build_coding_prompt(&config, repo.path());
    assert!(
        prompt.contains("Available Skills"),
        "prompt should list discovered skills"
    );
}

#[test]
fn coding_prompt_discovers_repo_local_skills() {
    let ws = TempDir::new().unwrap();
    let config = config::test_config(ws.path());
    let repo = TempDir::new().unwrap();

    // Create a repo-local skill
    let skill_dir = repo.path().join(".agents/skills/my-skill");
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    std::fs::write(
        skill_dir.join("skill.md"),
        "---\nname: my-repo-skill\ndescription: A repo-local skill.\n---\n\n# Body\n",
    )
    .expect("write skill.md");

    let prompt = coding::prompt::build_coding_prompt(&config, repo.path());
    assert!(
        prompt.contains("my-repo-skill"),
        "prompt should include repo-local skill"
    );
}

// ---------------------------------------------------------------------------
// chat_coding pipeline
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_coding_uses_custom_system_prompt() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db).await.expect("create session");

    let provider = Arc::new(MockProvider::new(vec![
        respond_response("coding response", vec![]),
    ]));
    let requests = provider.requests();

    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), config);
    let system_prompt = "You are a coding agent working in /tmp/repo.";

    let (result, _metadata) = chat
        .chat_coding(&session_id, "list files", system_prompt, None)
        .await
        .expect("chat_coding");

    assert_eq!(result.message, "coding response");
    assert_eq!(result.stop_reason, ChatStopReason::EndTurn);

    // Verify the custom system prompt was sent to the provider
    let recorded = requests.lock().expect("lock");
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].system.as_deref(),
        Some("You are a coding agent working in /tmp/repo.")
    );
}

#[tokio::test]
async fn chat_coding_tool_loop_works() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db).await.expect("create session");

    let provider = Arc::new(MockProvider::new(vec![
        response(
            vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "echo_tool".to_string(),
                input: json!({"text": "hello"}),
            }],
            StopReason::ToolUse,
        ),
        respond_response("tool done", vec![]),
    ]));
    let requests = provider.requests();

    let mut tools = ToolManager::empty();
    tools.register(Arc::new(EchoTool));
    let chat = SessionChat::new(db.clone(), provider, tools, config);

    let (result, metadata) = chat
        .chat_coding(&session_id, "run tool", "system prompt", None)
        .await
        .expect("chat_coding with tool");

    assert_eq!(result.message, "tool done");
    assert!(metadata.iterations >= 1, "should have at least 1 tool iteration");

    // Verify tool result was sent back
    let recorded = requests.lock().expect("lock");
    assert_eq!(recorded.len(), 2);
    let has_tool_result = recorded[1].messages.iter().any(|msg| {
        msg.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { content, is_error: false, .. }
                if content == "echo:hello"
            )
        })
    });
    assert!(has_tool_result, "second request should contain echo tool result");
}

#[tokio::test]
async fn chat_coding_persists_messages_to_db() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db).await.expect("create session");

    let provider = Arc::new(MockProvider::new(vec![
        respond_response("I updated the file.", vec![]),
    ]));
    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), config);

    let _ = chat
        .chat_coding(&session_id, "fix the bug", "coding system prompt", None)
        .await
        .expect("chat_coding");

    let messages = db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("list messages");

    // Should have: user message + assistant response
    assert!(messages.len() >= 2, "should have at least 2 messages, got {}", messages.len());
    assert_eq!(messages[0].role, "user");
    assert!(messages[0].content.contains("fix the bug"));
    assert_eq!(messages.last().unwrap().role, "assistant");
    assert!(messages.last().unwrap().content.contains("I updated the file."));
}

// ---------------------------------------------------------------------------
// cwd_override
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cwd_override_affects_tool_execution() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db).await.expect("create session");

    // Write a file inside the workspace so path resolution passes
    let subdir = config.workspace.join("repo");
    std::fs::create_dir_all(&subdir).expect("create repo subdir");
    std::fs::write(subdir.join("test.txt"), "hello from repo").expect("write test file");

    let provider = Arc::new(MockProvider::new(vec![
        response(
            vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                input: json!({"path": "test.txt"}),
            }],
            StopReason::ToolUse,
        ),
        respond_response("read it", vec![]),
    ]));
    let requests = provider.requests();

    // cwd_override points to workspace/repo — read_file("test.txt") resolves there
    let chat = SessionChat::new(db.clone(), provider, ToolManager::for_chat(), config)
        .with_cwd_override(subdir.clone());

    let _ = chat
        .chat_coding(&session_id, "read test.txt", "system prompt", None)
        .await
        .expect("chat_coding with cwd");

    // The tool result should contain the file content from the cwd dir
    let recorded = requests.lock().expect("lock");
    assert!(recorded.len() >= 2, "should have at least 2 requests");
    let has_file_content = recorded[1].messages.iter().any(|msg| {
        msg.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult { content, is_error: false, .. }
                if content.contains("hello from repo")
            )
        })
    });
    assert!(
        has_file_content,
        "tool should have read from cwd_override dir"
    );
}

// ---------------------------------------------------------------------------
// Full lifecycle: start → chat → end → resume
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_coding_session_lifecycle() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    // 1. Start a coding session
    let session = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("lifecycle-chan".to_string()),
        Some("add a greeting function".to_string()),
    )
    .await
    .expect("start");

    // Verify takeover is active
    let takeover = db::coding_sessions::get_active_takeover(&db, "lifecycle-chan")
        .await
        .expect("get takeover");
    assert!(takeover.is_some());

    // 2. Chat in the coding session
    let provider = Arc::new(MockProvider::new(vec![
        respond_response("I'll add the greeting function now.", vec![]),
    ]));
    let system_prompt =
        coding::prompt::build_coding_prompt(&config, repo.path());

    let chat = SessionChat::new(db.clone(), provider, ToolManager::empty(), config.clone())
        .with_cwd_override(repo.path().to_path_buf());

    let (result, _) = chat
        .chat_coding(
            &session.session_id,
            "add a greeting function",
            &system_prompt,
            None,
        )
        .await
        .expect("chat_coding");
    assert!(!result.message.is_empty());

    // 3. Make a commit in the repo (simulating agent work)
    tokio::fs::write(repo.path().join("greet.rs"), "pub fn greet() { println!(\"hi\"); }\n")
        .await
        .expect("write file");
    tokio::process::Command::new("git")
        .args(["add", "."])
        .current_dir(repo.path())
        .output()
        .await
        .expect("git add");
    tokio::process::Command::new("git")
        .args(["commit", "-m", "feat: add greeting function"])
        .current_dir(repo.path())
        .output()
        .await
        .expect("git commit");

    // 4. End the session
    let summary = coding::session::end(&db, &session.id, repo.path())
        .await
        .expect("end");
    assert!(
        summary.contains("greeting"),
        "summary should mention the commit: {summary}"
    );

    // Verify takeover is cleared
    let takeover = db::coding_sessions::get_active_takeover(&db, "lifecycle-chan")
        .await
        .expect("get takeover");
    assert!(takeover.is_none());

    // 5. Resume the session
    db::coding_sessions::reactivate_coding_session(&db, &session.id, Some("new-chan"))
        .await
        .expect("reactivate");

    let takeover = db::coding_sessions::get_active_takeover(&db, "new-chan")
        .await
        .expect("get takeover on new channel");
    assert!(takeover.is_some());
    let (id, sid, _) = takeover.unwrap();
    assert_eq!(id, session.id);
    assert_eq!(sid, session.session_id);

    // 6. Chat again on the resumed session
    let provider2 = Arc::new(MockProvider::new(vec![
        respond_response("Resumed and ready.", vec![]),
    ]));
    let chat2 = SessionChat::new(db.clone(), provider2, ToolManager::empty(), config)
        .with_cwd_override(repo.path().to_path_buf());

    let (result2, _) = chat2
        .chat_coding(
            &session.session_id,
            "what did we do last time?",
            &system_prompt,
            None,
        )
        .await
        .expect("resumed chat_coding");
    assert!(!result2.message.is_empty());

    // Verify all messages are in the same session
    let messages = db::sessions::list_messages_by_session(&db, &session.session_id)
        .await
        .expect("list messages");
    // initial prompt + response + resumed user + resumed response = at least 4
    assert!(
        messages.len() >= 4,
        "should have at least 4 messages across lifecycle, got {}",
        messages.len()
    );
}

// ---------------------------------------------------------------------------
// Takeover routing logic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn takeover_routes_by_channel_id() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    // Start two sessions on different channels
    let s1 = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("chan-A".to_string()),
        None,
    )
    .await
    .expect("start s1");

    let s2 = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("chan-B".to_string()),
        None,
    )
    .await
    .expect("start s2");

    // Each channel routes to its own session
    let t1 = db::coding_sessions::get_active_takeover(&db, "chan-A")
        .await
        .expect("takeover A");
    let t2 = db::coding_sessions::get_active_takeover(&db, "chan-B")
        .await
        .expect("takeover B");

    assert_eq!(t1.unwrap().0, s1.id);
    assert_eq!(t2.unwrap().0, s2.id);

    // Non-existent channel has no takeover
    let t3 = db::coding_sessions::get_active_takeover(&db, "chan-C")
        .await
        .expect("takeover C");
    assert!(t3.is_none());
}

#[tokio::test]
async fn ending_one_session_doesnt_affect_other() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    let s1 = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("chan-X".to_string()),
        None,
    )
    .await
    .expect("start s1");

    let _s2 = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("chan-Y".to_string()),
        None,
    )
    .await
    .expect("start s2");

    // End s1
    coding::session::end(&db, &s1.id, repo.path())
        .await
        .expect("end s1");

    // s2 should still be active
    let takeover = db::coding_sessions::get_active_takeover(&db, "chan-Y")
        .await
        .expect("takeover Y");
    assert!(takeover.is_some(), "s2 should still be active after ending s1");
}

// ---------------------------------------------------------------------------
// Listing sessions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_coding_sessions_returns_all() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    for i in 0..3 {
        coding::session::start(
            &db,
            &config,
            repo.path().to_path_buf(),
            Some(format!("chan-list-{i}")),
            None,
        )
        .await
        .expect("start");
    }

    let list = db::coding_sessions::list_recent_coding_sessions(&db, 10)
        .await
        .expect("list sessions");
    assert_eq!(list.len(), 3);
}

// ---------------------------------------------------------------------------
// Entry banner logic (message count check)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn entry_banner_fires_on_first_message() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    let session = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("banner-chan".to_string()),
        Some("initial prompt".to_string()),
    )
    .await
    .expect("start");

    // With initial prompt: count = 1, should show banner (count <= 1)
    let count = db::sessions::count_messages_for_session(&db, &session.session_id)
        .await
        .expect("count messages");
    assert_eq!(count, 1, "should have exactly 1 message (the initial prompt)");
    assert!(count <= 1, "banner condition should be true");

    // After a chat turn, count > 1, banner should not fire
    db::sessions::create_message(&db, &session.session_id, "user", "hello")
        .await
        .expect("add message");
    db::sessions::create_message(&db, &session.session_id, "assistant", "hi")
        .await
        .expect("add message");

    let count = db::sessions::count_messages_for_session(&db, &session.session_id)
        .await
        .expect("count messages");
    assert!(count > 1, "banner condition should be false after chat");
}

#[tokio::test]
async fn entry_banner_fires_without_initial_prompt() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let repo = create_mock_repo().await;

    let session = coding::session::start(
        &db,
        &config,
        repo.path().to_path_buf(),
        Some("banner-no-prompt".to_string()),
        None,
    )
    .await
    .expect("start");

    // Without initial prompt: count = 0, should still show banner (count <= 1)
    let count = db::sessions::count_messages_for_session(&db, &session.session_id)
        .await
        .expect("count messages");
    assert_eq!(count, 0);
    assert!(count <= 1, "banner condition should be true with 0 messages");
}
