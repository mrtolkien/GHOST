use std::path::Path;

use chrono::Utc;
use reqwest::header::HeaderMap;
use serde_json::{Map, Value, json};

use super::types::DebugContext;

pub struct DebugRequestData<'a> {
    pub dir: &'a Path,
    pub provider_name: &'a str,
    pub model: &'a str,
    pub request_body: &'a str,
    pub response_body: Option<&'a str>,
    pub status: u16,
    pub duration_ms: u64,
    pub response_headers: Option<&'a HeaderMap>,
    pub response_read_error: Option<&'a str>,
    pub stage: &'a str,
    pub debug_context: Option<&'a DebugContext>,
    pub max_saved_requests: usize,
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

    let payload = build_debug_payload(data, &now, iteration);

    if let Err(error) = std::fs::create_dir_all(data.dir) {
        tracing::warn!(
            dir = data.dir.display().to_string(),
            error = error.to_string(),
            "failed to create debug request dir",
        );
        return;
    }

    let path = data.dir.join(&filename);
    let json_str = serde_json::to_string_pretty(&payload)
        .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {e}\"}}"));

    if let Err(error) = std::fs::write(&path, &json_str) {
        tracing::warn!(
            path = path.display().to_string(),
            error = error.to_string(),
            "failed to write debug request file",
        );
    }

    if data.max_saved_requests > 0 {
        prune_old_requests(data.dir, data.max_saved_requests);
    }
}

fn build_debug_payload(
    data: &DebugRequestData<'_>,
    now: &chrono::DateTime<Utc>,
    iteration: usize,
) -> Value {
    let request_value: Value = serde_json::from_str(data.request_body)
        .unwrap_or_else(|_| Value::String(data.request_body.into()));
    let response_value = data
        .response_body
        .map(|body| serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_string())));

    json!({
        "timestamp": now.to_rfc3339(),
        "provider": data.provider_name,
        "model": data.model,
        "session_id": data.debug_context.map(|c| c.session_id.as_str()).unwrap_or("unknown"),
        "iteration": iteration,
        "status": data.status,
        "duration_ms": data.duration_ms,
        "stage": data.stage,
        "request": request_value,
        "response": response_value,
        "response_headers": data.response_headers.map(header_map_to_json),
        "response_read_error": data.response_read_error,
    })
}

fn header_map_to_json(headers: &HeaderMap) -> Value {
    let mut object = Map::new();
    for (name, value) in headers {
        let value = value
            .to_str()
            .map(ToString::to_string)
            .unwrap_or_else(|_| format!("<non-utf8:{} bytes>", value.as_bytes().len()));
        object.insert(name.as_str().to_string(), Value::String(value));
    }
    Value::Object(object)
}

fn prune_old_requests(dir: &Path, max: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut files: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .map(|e| e.path())
        .collect();

    if files.len() <= max {
        return;
    }

    // Filenames are timestamp-prefixed, so alphabetical sort = chronological order.
    files.sort();

    let to_remove = files.len() - max;
    for path in &files[..to_remove] {
        if let Err(error) = std::fs::remove_file(path) {
            tracing::warn!(
                path = path.display().to_string(),
                error = error.to_string(),
                "failed to prune debug request file",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue};

    use super::*;

    #[test]
    fn builds_payload_for_body_read_failure() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            HeaderValue::from_static("text/event-stream"),
        );
        headers.insert("x-codex-turn-state", HeaderValue::from_static("turn-123"));
        let debug_context = DebugContext {
            session_id: "session-abc".to_string(),
            iteration: 7,
        };
        let now = Utc::now();
        let payload = build_debug_payload(
            &DebugRequestData {
                dir: Path::new("/tmp"),
                provider_name: "openai_oauth",
                model: "gpt-5.4",
                request_body: r#"{"hello":"world"}"#,
                response_body: None,
                status: 200,
                duration_ms: 1234,
                response_headers: Some(&headers),
                response_read_error: Some("error decoding response body"),
                stage: "body_read_failed",
                debug_context: Some(&debug_context),
                max_saved_requests: 0,
            },
            &now,
            debug_context.iteration,
        );

        assert_eq!(payload["stage"], "body_read_failed");
        assert_eq!(payload["status"], 200);
        assert_eq!(payload["response"], Value::Null);
        assert_eq!(
            payload["response_read_error"],
            "error decoding response body"
        );
        assert_eq!(
            payload["response_headers"]["content-type"],
            "text/event-stream"
        );
        assert_eq!(
            payload["response_headers"]["x-codex-turn-state"],
            "turn-123"
        );
    }
}
