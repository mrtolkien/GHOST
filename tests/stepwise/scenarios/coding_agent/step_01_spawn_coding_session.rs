use std::time::Duration;

use crate::common;
use crate::stepwise::harness;

/// Step 01: GHOST receives a coding request, reads the coding skill, and
/// spawns a coding session via `ghost hack start`.
///
/// Pre-conditions:
/// - A git repo exists at `code/test-repo/` in the workspace
/// - A `repo.md` reference exists pointing to it
///
/// Assertions:
/// - GHOST calls `shell` with `ghost hack start`
/// - A coding session record exists in the DB
/// - The coding session has status "active"
#[tokio::test]
async fn coding_agent_step_01_spawn_coding_session() {
    let env = common::live_test_database("coding_agent_step_01").await;

    // Set up a git repo at code/test-repo/ in the workspace
    let repo_dir = env.workspace_path().join("code/test-repo");
    std::fs::create_dir_all(&repo_dir).expect("create code/test-repo/");

    // Initialize git repo with a file the coding agent will later edit
    run_cmd(&repo_dir, "git", &["init"]);
    run_cmd(&repo_dir, "git", &["config", "user.email", "test@test.com"]);
    run_cmd(&repo_dir, "git", &["config", "user.name", "Test"]);
    std::fs::write(
        repo_dir.join("greeting.py"),
        "def greet():\n    return \"hello\"\n",
    )
    .expect("write greeting.py");
    run_cmd(&repo_dir, "git", &["add", "."]);
    run_cmd(
        &repo_dir,
        "git",
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "initial commit",
        ],
    );

    // Create a reference note so GHOST knows about the repo
    common::write_test_reference(
        env.workspace_path(),
        "test-repo",
        "repo.md",
        "# test-repo\n\n\
        Local path: code/test-repo\n\
        Build: python3 greeting.py\n\
        Test: python3 -c \"from greeting import greet; assert greet() == 'hello'\"\n",
    );

    let session = env.create_session().await;
    let chat = env.chat();

    // Ask GHOST to hack on the repo — this should trigger the coding skill
    let (result, _metadata) = tokio::time::timeout(
        Duration::from_secs(180),
        chat.chat(
            &session,
            "Hack on test-repo. The repo is already cloned at code/test-repo. \
             Start a coding session so I can work on it. \
             Use '--prompt \"change the greeting function to 'hello world'\"' as the initial prompt.",
            None,
            None,
        ),
    )
    .await
    .expect("TIMEOUT: GHOST should respond within 180s")
    .expect("GHOST chat failed in step_01");

    env.log_session_json("ghost_chat", &session).await;

    assert!(
        !result.message.trim().is_empty(),
        "GHOST should respond with a non-empty message"
    );

    // Verify GHOST called shell with ghost hack start
    let messages = ghost::db::sessions::list_messages_by_session(&env.db, &session)
        .await
        .expect("list messages");

    let has_hack_start = messages.iter().any(|msg| {
        msg.tool_calls_parsed()
            .map(|calls| {
                calls.iter().any(|c| {
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                    let input = c
                        .get("input")
                        .and_then(|v| v.as_object())
                        .cloned()
                        .unwrap_or_default();
                    name == "shell"
                        && input
                            .get("command")
                            .and_then(|v| v.as_str())
                            .is_some_and(|cmd| cmd.contains("ghost hack start"))
                })
            })
            .unwrap_or(false)
    });
    assert!(
        has_hack_start,
        "GHOST should call `ghost hack start` via shell"
    );

    // Verify a coding session was created in the DB
    let coding_sessions = ghost::db::coding_sessions::list_recent_coding_sessions(&env.db, 10)
        .await
        .expect("list coding sessions");
    assert!(
        !coding_sessions.is_empty(),
        "at least one coding session should exist after ghost hack start"
    );

    let (coding_id, coding_session_id, working_dir, status, _started_at) = &coding_sessions[0];
    assert_eq!(status, "active", "coding session should be active");
    assert!(
        working_dir.contains("test-repo"),
        "working_dir should reference test-repo: {working_dir}"
    );

    // Store working_dir as relative to workspace so step 02 can resolve it
    // against its own restored workspace (which has a different temp path).
    let workspace_str = env
        .workspace_path()
        .to_str()
        .expect("workspace path is utf-8");
    let relative_working_dir = working_dir
        .strip_prefix(workspace_str)
        .map(|s| s.trim_start_matches('/'))
        .unwrap_or(working_dir);

    // Save state for step 02
    let mut state = harness::fresh_step_state(
        harness::SCENARIO_CODING_AGENT,
        harness::STEP_CA_01,
        None,
        session,
    );
    state.assertion_markers.insert(
        "coding_session_id".to_string(),
        serde_json::json!(coding_id),
    );
    state.assertion_markers.insert(
        "coding_chat_session_id".to_string(),
        serde_json::json!(coding_session_id),
    );
    state.assertion_markers.insert(
        "working_dir".to_string(),
        serde_json::json!(relative_working_dir),
    );

    harness::save_step_snapshot(&env, &state).await;
}

fn run_cmd(dir: &std::path::Path, cmd: &str, args: &[&str]) {
    std::process::Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("{cmd} {:?} failed: {e}", args));
}
