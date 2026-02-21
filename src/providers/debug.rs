use std::path::Path;

use chrono::Utc;
use serde_json::{Value, json};

use super::types::DebugContext;

pub struct DebugRequestData<'a> {
    pub dir: &'a Path,
    pub provider_name: &'a str,
    pub model: &'a str,
    pub request_body: &'a str,
    pub response_body: &'a str,
    pub status: u16,
    pub duration_ms: u64,
    pub debug_context: Option<&'a DebugContext>,
}

pub fn save_debug_request(data: &DebugRequestData<'_>) {
    let now = Utc::now();
    let timestamp = now.format("%Y%m%dT%H%M%S%.3f");

    let (session_short, iteration) = match data.debug_context {
        Some(ctx) => {
            let short = if ctx.session_id.len() > 8 {
                &ctx.session_id[ctx.session_id.len() - 8..]
            } else {
                &ctx.session_id
            };
            (short.to_string(), ctx.iteration)
        }
        None => ("unknown".to_string(), 0),
    };

    let filename = format!("{timestamp}_{session_short}_{iteration}.json");

    let request_value: Value = serde_json::from_str(data.request_body)
        .unwrap_or_else(|_| Value::String(data.request_body.into()));
    let response_value: Value = serde_json::from_str(data.response_body)
        .unwrap_or_else(|_| Value::String(data.response_body.into()));

    let payload = json!({
        "timestamp": now.to_rfc3339(),
        "provider": data.provider_name,
        "model": data.model,
        "session_id": data.debug_context.map(|c| c.session_id.as_str()).unwrap_or("unknown"),
        "iteration": iteration,
        "status": data.status,
        "duration_ms": data.duration_ms,
        "request": request_value,
        "response": response_value,
    });

    if let Err(error) = std::fs::create_dir_all(data.dir) {
        logfire::warn!(
            "failed to create debug request dir",
            dir = data.dir.display().to_string(),
            error = error.to_string(),
        );
        return;
    }

    let path = data.dir.join(&filename);
    let json_str = serde_json::to_string_pretty(&payload)
        .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {e}\"}}"));

    if let Err(error) = std::fs::write(&path, &json_str) {
        logfire::warn!(
            "failed to write debug request file",
            path = path.display().to_string(),
            error = error.to_string(),
        );
    }
}
