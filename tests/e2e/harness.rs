use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::common;

pub const SCENARIO_PRINTER_3D: &str = "printer_3d";
pub const STEP_01: &str = "step_01_spawn_agent";
pub const STEP_02: &str = "step_02_run_agent_completion";
pub const STEP_03: &str = "step_03_reflect_agent";
pub const STEP_04: &str = "step_04_finalize_chat_and_reflect";

const STATE_FILE: &str = "state.json";
const ARCHIVE_FILE: &str = "workspace.tar.zst";
const TRANSCRIPT_JSON_FILE: &str = "transcript.json";
const TRANSCRIPT_MD_FILE: &str = "transcript.md";
const METRICS_FILE: &str = "metrics.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepState {
    pub schema_version: u32,
    pub scenario: String,
    pub model_alias: String,
    pub step: String,
    pub created_at: String,
    pub parent_step: Option<String>,
    pub chat_session_id: String,
    pub agent_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub final_response_preview: Option<String>,
    pub assertion_markers: BTreeMap<String, serde_json::Value>,
}

pub struct LoadedStep {
    pub env: common::LiveTestEnv,
    pub state: StepState,
}

pub fn model_alias() -> String {
    if let Ok(model) = std::env::var("GHOST_E2E_MODEL") {
        return model;
    }

    default_model_alias_from_config().unwrap_or_else(|| "primary".to_string())
}

pub fn scenario_model_dir(scenario: &str, model: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("e2e")
        .join(scenario)
        .join(model)
}

pub fn step_dir(scenario: &str, model: &str, step: &str) -> PathBuf {
    scenario_model_dir(scenario, model).join(step)
}

pub async fn load_previous_step_or_fail(
    scenario: &str,
    current_step: &str,
    previous_step: &str,
) -> LoadedStep {
    let model = model_alias();
    let previous_dir = step_dir(scenario, &model, previous_step);
    let state_path = previous_dir.join(STATE_FILE);
    let archive_path = previous_dir.join(ARCHIVE_FILE);
    let available_models = available_models_for_scenario(scenario);

    assert!(
        state_path.exists(),
        "missing predecessor state for {scenario}/{model}/{current_step}: {}. available models: {:?}",
        state_path.display(),
        available_models
    );
    assert!(
        archive_path.exists(),
        "missing predecessor archive for {scenario}/{model}/{current_step}: {}. available models: {:?}",
        archive_path.display(),
        available_models
    );

    let raw = fs::read_to_string(&state_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", state_path.display()));
    let state: StepState = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse {}: {e}", state_path.display()));

    let env_name = format!("{scenario}_{current_step}");
    let env = common::live_test_database_from_snapshot(&env_name, Some(&archive_path)).await;

    LoadedStep { env, state }
}

fn default_model_alias_from_config() -> Option<String> {
    let config_dir = std::env::var_os(ghost::config::CONFIG_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/ghost")))?;
    let raw = fs::read_to_string(config_dir.join("config.toml")).ok()?;
    let value: toml::Value = toml::from_str(&raw).ok()?;
    value
        .get("models")
        .and_then(|v| v.get("default"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

fn available_models_for_scenario(scenario: &str) -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("e2e")
        .join(scenario);
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            if e.file_type().ok()?.is_dir() {
                e.file_name().into_string().ok()
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

pub async fn save_step_snapshot(env: &common::LiveTestEnv, state: &StepState) {
    let dir = step_dir(&state.scenario, &state.model_alias, &state.step);
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir {}: {e}", dir.display()));

    let archive_path = dir.join(ARCHIVE_FILE);
    env.write_workspace_archive(&archive_path);

    let state_path = dir.join(STATE_FILE);
    let state_json =
        serde_json::to_string_pretty(state).unwrap_or_else(|e| panic!("serialize state: {e}"));
    fs::write(&state_path, state_json)
        .unwrap_or_else(|e| panic!("write {}: {e}", state_path.display()));

    let transcript_json = build_transcript_json(
        env,
        &state.chat_session_id,
        state.agent_session_id.as_deref(),
    )
    .await;
    let transcript_json_path = dir.join(TRANSCRIPT_JSON_FILE);
    fs::write(
        &transcript_json_path,
        serde_json::to_string_pretty(&transcript_json)
            .unwrap_or_else(|e| panic!("serialize transcript: {e}")),
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", transcript_json_path.display()));

    let transcript_md_path = dir.join(TRANSCRIPT_MD_FILE);
    fs::write(
        &transcript_md_path,
        render_transcript_markdown(&transcript_json),
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", transcript_md_path.display()));

    let metrics = build_metrics(
        env,
        &state.chat_session_id,
        state.agent_session_id.as_deref(),
    )
    .await;
    let metrics_path = dir.join(METRICS_FILE);
    fs::write(
        &metrics_path,
        serde_json::to_string_pretty(&metrics).unwrap_or_else(|e| panic!("serialize metrics: {e}")),
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", metrics_path.display()));
}

pub fn fresh_step_state(
    scenario: &str,
    step: &str,
    parent_step: Option<&str>,
    chat_session_id: String,
) -> StepState {
    StepState {
        schema_version: 1,
        scenario: scenario.to_string(),
        model_alias: model_alias(),
        step: step.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        parent_step: parent_step.map(str::to_string),
        chat_session_id,
        agent_id: None,
        agent_session_id: None,
        final_response_preview: None,
        assertion_markers: BTreeMap::new(),
    }
}

async fn build_transcript_json(
    env: &common::LiveTestEnv,
    chat_session_id: &str,
    agent_session_id: Option<&str>,
) -> serde_json::Value {
    let chat = env.collect_session_json(chat_session_id).await;
    let agent = if let Some(agent_session_id) = agent_session_id {
        env.collect_session_json(agent_session_id).await
    } else {
        Vec::new()
    };
    json!({
        "chat_session_id": chat_session_id,
        "agent_session_id": agent_session_id,
        "chat": chat,
        "agent": agent,
    })
}

async fn build_metrics(
    env: &common::LiveTestEnv,
    chat_session_id: &str,
    agent_session_id: Option<&str>,
) -> serde_json::Value {
    let chat_tools = env.collect_tool_calls(chat_session_id).await;
    let mut metrics = json!({
        "chat_tool_calls": chat_tools,
    });

    if let Some(agent_session_id) = agent_session_id {
        let wf = env.collect_web_fetch_metrics(agent_session_id).await;
        metrics["agent_web_fetch_count"] = json!(wf.count);
        metrics["agent_web_fetch_urls"] = json!(wf.urls);
    }

    metrics
}

fn render_transcript_markdown(transcript: &serde_json::Value) -> String {
    let mut out = String::new();
    out.push_str("# E2E Transcript\n\n");

    render_section(&mut out, "Chat", transcript.get("chat"));
    render_section(&mut out, "Agent", transcript.get("agent"));

    out
}

fn render_section(out: &mut String, title: &str, maybe_messages: Option<&serde_json::Value>) {
    out.push_str(&format!("## {title}\n\n"));
    let Some(messages) = maybe_messages.and_then(|v| v.as_array()) else {
        out.push_str("_No messages_\n\n");
        return;
    };

    for (idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("?");
        out.push_str(&format!("### {}. {}\n\n", idx + 1, role));

        if let Some(content) = msg.get("content").and_then(|v| v.as_str())
            && !content.trim().is_empty()
        {
            out.push_str("**Content**\n\n");
            out.push_str("```text\n");
            out.push_str(content);
            out.push_str("\n```\n\n");
        }

        if let Some(raw) = msg.get("raw_output").and_then(|v| v.as_array())
            && !raw.is_empty()
        {
            out.push_str("**Thinking / Raw Output**\n\n");
            for item in raw {
                let ty = item
                    .get("original_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let summary = item.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("- `{ty}`: {summary}\n"));
            }
            out.push('\n');
        }

        if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array())
            && !calls.is_empty()
        {
            out.push_str("**Tool Calls**\n\n");
            for call in calls {
                let name = call.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let input = call.get("input").cloned().unwrap_or(json!(null));
                out.push_str(&format!("- `{name}`\n"));
                out.push_str("```json\n");
                out.push_str(
                    &serde_json::to_string_pretty(&input).unwrap_or_else(|_| "null".to_string()),
                );
                out.push_str("\n```\n");
            }
            out.push('\n');
        }

        if let Some(results) = msg.get("tool_results").and_then(|v| v.as_array())
            && !results.is_empty()
        {
            out.push_str("**Tool Results**\n\n");
            for result in results {
                let is_error = result
                    .get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let content = result.get("content").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("- error={is_error}\n"));
                out.push_str("```text\n");
                out.push_str(content);
                out.push_str("\n```\n");
            }
            out.push('\n');
        }
    }
}
