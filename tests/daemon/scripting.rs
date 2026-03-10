use std::time::Duration;

use crate::helpers::live_test_database;

/// Helper: find a Python script in the workspace under scripts/{subdir}/.
fn find_script(env: &crate::common::LiveTestEnv, subdir: &str) -> Option<String> {
    let dir = env.workspace_path().join("scripts").join(subdir);
    if !dir.exists() {
        return None;
    }
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("py") {
            return std::fs::read_to_string(&path).ok();
        }
    }
    None
}

/// Assert standard scripting conventions on a Python script.
fn assert_script_conventions(content: &str, test_name: &str) {
    assert!(
        content.contains("# /// script"),
        "[{test_name}] missing PEP 723 metadata block"
    );
    assert!(
        content.contains("# ///"),
        "[{test_name}] missing PEP 723 closing marker"
    );
    assert!(
        content.contains("\"\"\""),
        "[{test_name}] missing module docstring"
    );
}

// ---------------------------------------------------------------------------
// US1: Monthly spending from bank CSV
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_script_csv_spending() {
    let env = live_test_database("script_csv_spending").await;
    let daemon = env.boot_daemon().await;

    // Copy the CSV fixture into the workspace
    let fixture = std::path::Path::new("tests/fixtures/mock_bank_statement.csv");
    let dest = env.workspace_path().join("bank_statement.csv");
    std::fs::copy(fixture, &dest).expect("copy CSV fixture");

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    let timeout = Duration::from_secs(180);
    tokio::time::timeout(timeout, async {
        daemon
            .session_chat
            .chat(
                &session_id,
                "Write me a reusable script that analyzes bank_statement.csv and breaks \
                 down spending by category. I want to run it monthly. Try it on the CSV \
                 in my workspace to show me this month's food spending (groceries vs restaurants).",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: script_csv_spending exceeded 180s");

    daemon.settle().await.expect("settle");

    // Assert: a script was created under scripts/
    let script_content = ["finance", "spending", "budget", "bank"]
        .iter()
        .find_map(|topic| find_script(&env, topic));

    let content = script_content
        .expect("expected a Python script under scripts/{finance,spending,budget,bank}/");
    assert_script_conventions(&content, "csv_spending");

    assert!(
        content.contains("typer"),
        "expected script to use typer for CLI arguments"
    );

    env.log_session_json("csv_spending", &session_id).await;
    daemon.shutdown().await;
}

// ---------------------------------------------------------------------------
// US2: Weather forecast
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_script_weather_forecast() {
    let env = live_test_database("script_weather_forecast").await;
    let daemon = env.boot_daemon().await;

    let session_id = ghost::db::sessions::create_session(&daemon.db)
        .await
        .expect("create session");

    let timeout = Duration::from_secs(180);
    tokio::time::timeout(timeout, async {
        daemon
            .session_chat
            .chat(
                &session_id,
                "Write me a script that fetches the weather forecast for Tokyo station, \
                 Tokyo. I'll be asking you for the weather regularly so I want a reusable script.",
                None,
                None,
            )
            .await
            .expect("chat failed");
    })
    .await
    .expect("TIMEOUT: script_weather_forecast exceeded 180s");

    daemon.settle().await.expect("settle");

    let script_content = ["weather", "forecast", "meteo"]
        .iter()
        .find_map(|topic| find_script(&env, topic));

    let content =
        script_content.expect("expected a Python script under scripts/{weather,forecast,meteo}/");
    assert_script_conventions(&content, "weather_forecast");

    assert!(
        content.contains("httpx") || content.contains("requests") || content.contains("urllib"),
        "expected script to use an HTTP client library"
    );

    env.log_session_json("weather_forecast", &session_id).await;
    daemon.shutdown().await;
}
