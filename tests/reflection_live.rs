#![cfg(feature = "live-tests-llms")]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

const GPT53_PRINTER_3D_STEP_02_DIR: &str =
    "tests/fixtures/e2e/printer_3d/gpt53/step_02_run_agent_completion";
const GEMMA4_LOCAL_MODEL_ALIAS: &str = "gemma4_local";
const REFLECTION_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Deserialize)]
struct FixtureState {
    agent_session_id: Option<String>,
    assertion_markers: BTreeMap<String, Value>,
}

#[tokio::test]
async fn printer_3d_structured_reflection_from_gpt53_snapshot() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GPT53_PRINTER_3D_STEP_02_DIR);
    let state_path = fixture_dir.join("state.json");
    let archive_path = fixture_dir.join("workspace.tar.zst");

    let state: FixtureState =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read fixture state.json"))
            .expect("parse fixture state.json");

    let source_agent_session_id = state
        .agent_session_id
        .as_deref()
        .expect("fixture state must include agent_session_id");
    let report_data = state
        .assertion_markers
        .get("report_data")
        .and_then(Value::as_str)
        .expect("fixture state must include report_data");

    // SAFETY: set during single test setup before async work starts.
    unsafe { std::env::set_var("GHOST_E2E_MODEL", GEMMA4_LOCAL_MODEL_ALIAS) };

    let env = common::live_test_database("printer_3d_structured_reflection_gpt53_input").await;
    restore_workspace_without_database(env.workspace_path(), &archive_path)
        .expect("restore workspace archive without database");
    ghost::bundled::install_all(env.workspace_path()).expect("reinstall current bundled files");

    let agent_session_id = ghost::db::sessions::create_agent_session(&env.db)
        .await
        .expect("create fresh agent session");
    copy_cache_for_session(
        env.workspace_path(),
        source_agent_session_id,
        &agent_session_id,
    )
    .expect("copy cached web artifacts for remapped agent session");

    let notes_before = env.list_notes().len();
    let refs_before = env.list_references().len();

    let (findings, _metadata) = tokio::time::timeout(
        Duration::from_secs(REFLECTION_TIMEOUT_SECS),
        Box::pin(env.run_structured_reflection(&agent_session_id, report_data)),
    )
    .await
    .expect("TIMEOUT: structured reflection did not complete");

    assert!(
        !findings.trim().is_empty(),
        "expected non-empty structured reflection findings"
    );

    let notes_after = env.list_notes();
    let refs_after = env.list_references();

    assert!(
        notes_after.len() > notes_before,
        "expected reflection to create notes (before={}, after={})",
        notes_before,
        notes_after.len()
    );
    assert!(
        refs_after.len() > refs_before,
        "expected reflection to curate references (before={}, after={})",
        refs_before,
        refs_after.len()
    );

    env.assert_notes_contain_any(
        &["Prusa", "Bambu", "QIDI", "Creality", "printer"],
        "structured reflection should write note-taking output for the printer research",
    );
}

fn restore_workspace_without_database(workspace: &std::path::Path, snapshot: &std::path::Path) -> std::io::Result<()> {
    let file = fs::File::open(snapshot)?;
    let decoder = zstd::stream::read::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if file_name == "ghost.db" || file_name == "ghost.db-wal" || file_name == "ghost.db-shm"
        {
            continue;
        }
        entry.unpack_in(workspace)?;
    }

    Ok(())
}

fn copy_cache_for_session(
    workspace: &std::path::Path,
    source_session_id: &str,
    dest_session_id: &str,
) -> std::io::Result<()> {
    let source = workspace.join(".cache").join(source_session_id);
    let dest = workspace.join(".cache").join(dest_session_id);

    if !source.exists() {
        return Ok(());
    }

    copy_dir_all(&source, &dest)
}

fn copy_dir_all(source: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&source_path, &dest_path)?;
        } else {
            fs::copy(source_path, dest_path)?;
        }
    }
    Ok(())
}
