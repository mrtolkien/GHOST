use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;
use crate::web::browser::BrowserManager;
use crate::web::browser::cdp::ScrollDirection;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;
use super::output::ToolOutput;

/// Mount path inside the Chrome container where workspace/uploads/ is mapped.
const CHROME_UPLOADS_MOUNT: &str = "/uploads";

#[derive(Debug)]
pub struct BrowserTool;

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser".to_string(),
            description: "Control a headless browser for pages requiring interaction \
                (login walls, forms, JS-heavy content). Use web_fetch for simple page \
                reads — after logging in via browser, web_fetch can access authenticated \
                content (shared cookies). Workflow: call snapshot to get the page's \
                accessibility tree as XML with ref IDs (e.g. ref=\"e5\"), then use refs \
                to interact (click, type, fill, press). Refs are invalidated on each \
                snapshot call — always re-snapshot after actions that change the page \
                (navigate, click that triggers navigation)."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "navigate", "snapshot", "click",
                            "type", "scroll", "screenshot",
                            "press", "hover", "select",
                            "fill", "wait", "evaluate",
                            "drag", "resize", "upload",
                            "tabs", "open", "focus", "close",
                            "browsers", "connect", "disconnect",
                            "discover"
                        ],
                        "description": "navigate: open URL (replaces current page). \
                            snapshot: get accessibility tree with ref IDs. \
                            click: click element by ref. \
                            type: enter text into element by ref. \
                            scroll: scroll page or element. \
                            screenshot: capture page as PNG image. \
                            press: send a keyboard key (Enter, Escape, Tab, etc.). \
                            hover: hover over element by ref. \
                            select: select option in <select> dropdown by ref. \
                            fill: fill multiple form fields at once. \
                            wait: wait for a fixed duration (timeout param) or for a ref to be DOM-resolvable (ref must be from current snapshot). \
                            evaluate: execute JavaScript expression. \
                            drag: drag element to another element. \
                            resize: resize the browser viewport. \
                            upload: set file(s) on a <input type='file'> by ref. Requires 'path' parameter. \
                            tabs: list open tabs in the active browser. \
                            open: open a new tab, optionally navigating to 'url'. Returns snapshot. \
                            focus: switch to tab by ID ('tab' param). Returns snapshot with fresh refs. \
                            close: close tab by ID ('tab' param). \
                            browsers: list all known browsers with connection status. \
                            connect: connect to a browser by 'name' and 'cdp_url', set as active. \
                            disconnect: disconnect from a browser by 'name'. \
                            discover: scan localhost and Tailscale peers for CDP endpoints."
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to. Required for 'navigate'."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Element ref ID from snapshot (e.g. 'e5'). \
                            Required for 'click', 'type', 'hover', 'select', 'fill', \
                            'drag'. Optional for 'scroll' (scrolls element into view)."
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type. Required for 'type'."
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down"],
                        "description": "Scroll direction. Defaults to 'down'. \
                            Only for 'scroll'."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Skip first N nodes in snapshot. For \
                            paginating large trees. Only for 'snapshot'."
                    },
                    "key": {
                        "type": "string",
                        "description": "Key to press (e.g. 'Enter', 'Escape', \
                            'Tab', 'ArrowDown'). Required for 'press'."
                    },
                    "value": {
                        "type": "string",
                        "description": "Option value for 'select'. Used by \
                            'select' action."
                    },
                    "fields": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "ref": {"type": "string"},
                                "value": {"type": "string"}
                            },
                            "required": ["ref", "value"]
                        },
                        "description": "Array of {ref, value} pairs for 'fill' \
                            action."
                    },
                    "expression": {
                        "type": "string",
                        "description": "JavaScript expression to evaluate. \
                            Required for 'evaluate'."
                    },
                    "target_ref": {
                        "type": "string",
                        "description": "Target element ref for 'drag' action."
                    },
                    "width": {
                        "type": "integer",
                        "description": "Viewport width in pixels. Required for \
                            'resize'."
                    },
                    "height": {
                        "type": "integer",
                        "description": "Viewport height in pixels. Required for \
                            'resize'."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Wait timeout in milliseconds. Defaults \
                            to 1000. For 'wait' action."
                    },
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative file path for 'upload' \
                            action (e.g. 'uploads/1710504000_data.csv')."
                    },
                    "tab": {
                        "type": "integer",
                        "description": "Tab ID for 'focus' and 'close' actions."
                    },
                    "name": {
                        "type": "string",
                        "description": "Browser name for 'connect' and 'disconnect' actions."
                    },
                    "cdp_url": {
                        "type": "string",
                        "description": "WebSocket CDP URL for 'connect' action \
                            (e.g. 'ws://localhost:9222')."
                    }
                },
                "required": ["action"]
            }),
        }
    }

    #[tracing::instrument(skip_all, fields(tool = "browser"))]
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = params
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("missing required parameter: action".into()))?;

        let mut mgr = ctx.browser_manager.lock().await;

        match action {
            "navigate" => execute_navigate(&mut mgr, &params).await,
            "snapshot" => execute_snapshot(&mut mgr, &params).await,
            "click" => execute_click(&mut mgr, &params).await,
            "type" => execute_type(&mut mgr, &params).await,
            "scroll" => execute_scroll(&mut mgr, &params).await,
            "screenshot" => execute_screenshot(&mut mgr, &params, ctx).await,
            "press" => execute_press(&mut mgr, &params).await,
            "hover" => execute_hover(&mut mgr, &params).await,
            "select" => execute_select(&mut mgr, &params).await,
            "fill" => execute_fill(&mut mgr, &params).await,
            "wait" => execute_wait(&mut mgr, &params).await,
            "evaluate" => execute_evaluate(&mut mgr, &params).await,
            "drag" => execute_drag(&mut mgr, &params).await,
            "resize" => execute_resize(&mut mgr, &params).await,
            "upload" => execute_upload(&mut mgr, &params, ctx).await,
            "tabs" => execute_tabs(&mut mgr).await,
            "open" => execute_open(&mut mgr, &params).await,
            "focus" => execute_focus(&mut mgr, &params).await,
            "close" => execute_close(&mut mgr, &params).await,
            "browsers" => execute_browsers(&mut mgr).await,
            "connect" => execute_connect(&mut mgr, &params).await,
            "disconnect" => execute_disconnect(&mut mgr, &params).await,
            "discover" => execute_discover().await,
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action: {action}"
            ))),
        }
    }
}

async fn execute_navigate(
    mgr: &mut BrowserManager,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let url = params
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'navigate' requires 'url' parameter".into()))?;
    let (final_url, title) = mgr
        .navigate(url)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let result = json!({"ok": true, "url": final_url, "title": title});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_snapshot(
    mgr: &mut BrowserManager,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let xml = mgr
        .snapshot(offset)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let wrapped = format!(
        "<<<EXTERNAL_UNTRUSTED_CONTENT>>>\n\
         Source: Browser ({url})\n\
         ---\n\
         {xml}\n\
         <<<END_EXTERNAL_UNTRUSTED_CONTENT>>>"
    );
    Ok(ToolOutput::text(wrapped))
}

async fn execute_click(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'click' requires 'ref' parameter".into()))?;
    let desc = mgr
        .click(ref_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_type(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'type' requires 'ref' parameter".into()))?;
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'type' requires 'text' parameter".into()))?;
    let desc = mgr
        .type_text(ref_id, text)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_scroll(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let direction = match params.get("direction").and_then(Value::as_str) {
        Some("up") => ScrollDirection::Up,
        _ => ScrollDirection::Down,
    };
    let ref_id = params.get("ref").and_then(Value::as_str);
    let desc = mgr
        .scroll(direction, ref_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_screenshot(
    mgr: &mut BrowserManager,
    params: &Value,
    ctx: &ToolContext,
) -> Result<ToolOutput, ToolError> {
    let _ = params;
    let path = mgr
        .screenshot(&ctx.workspace)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let rel = path.strip_prefix(&ctx.workspace).unwrap_or(&path);
    let url = mgr.current_url().await.unwrap_or_default();
    let (vw, vh) = mgr.viewport_size();
    let result = json!({
        "ok": true,
        "url": url,
        "path": rel.display().to_string(),
        "description": format!("Screenshot captured ({vw}x{vh})")
    });
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_press(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'press' requires 'key' parameter".into()))?;
    let desc = mgr
        .press(key)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_hover(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'hover' requires 'ref' parameter".into()))?;
    let desc = mgr
        .hover(ref_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_select(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'select' requires 'ref' parameter".into()))?;
    let value = params
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'select' requires 'value' parameter".into()))?;
    let desc = mgr
        .select(ref_id, value)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_fill(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let fields_val = params
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::InvalidParams("'fill' requires 'fields' parameter".into()))?;

    let mut fields = Vec::with_capacity(fields_val.len());
    for item in fields_val {
        let ref_id = item
            .get("ref")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("each field must have a 'ref' key".into()))?
            .to_string();
        let value = item
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidParams("each field must have a 'value' key".into()))?
            .to_string();
        fields.push((ref_id, value));
    }

    let desc = mgr
        .fill(&fields)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_wait(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let ref_id = params.get("ref").and_then(Value::as_str);
    let timeout_ms = params
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(1000);
    let desc = mgr
        .wait(ref_id, timeout_ms)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_evaluate(
    mgr: &mut BrowserManager,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let expression = params
        .get("expression")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolError::InvalidParams("'evaluate' requires 'expression' parameter".into())
        })?;
    let result_str = mgr
        .evaluate(expression)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let wrapped = format!(
        "<<<EXTERNAL_UNTRUSTED_CONTENT>>>\n\
         Source: Browser JS ({url})\n\
         ---\n\
         {result_str}\n\
         <<<END_EXTERNAL_UNTRUSTED_CONTENT>>>"
    );
    let result = json!({"ok": true, "url": url, "result": wrapped});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_drag(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'drag' requires 'ref' parameter".into()))?;
    let target_ref = params
        .get("target_ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'drag' requires 'target_ref' parameter".into()))?;
    let desc = mgr
        .drag(ref_id, target_ref)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_resize(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let width = params
        .get("width")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'resize' requires 'width' parameter".into()))?
        as u32;
    let height = params
        .get("height")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'resize' requires 'height' parameter".into()))?
        as u32;
    let desc = mgr
        .resize(width, height)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

/// Resolve a workspace-relative path to the Chrome-container path.
///
/// Files already in `uploads/` map directly to `/uploads/...` inside the
/// container. Files elsewhere in the workspace are copied to a staging
/// directory under `uploads/.browser-staging/` so Chrome can see them.
/// Returns `(chrome_path, staging_host_path_if_copied)`.
async fn stage_for_chrome(
    workspace: &std::path::Path,
    rel_path: &str,
) -> Result<(String, Option<PathBuf>), ToolError> {
    let host_path = workspace.join(rel_path);

    // Security: ensure the resolved path stays inside the workspace.
    let canonical = tokio::fs::canonicalize(&host_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("file not found: {rel_path} ({e})")))?;
    let ws_canonical = tokio::fs::canonicalize(workspace)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("workspace error: {e}")))?;
    if !canonical.starts_with(&ws_canonical) {
        return Err(ToolError::ExecutionFailed(
            "path escapes workspace boundary".into(),
        ));
    }

    // If the file is already under uploads/, Chrome can see it directly.
    let uploads_dir = workspace.join("uploads");
    if canonical.starts_with(
        tokio::fs::canonicalize(&uploads_dir)
            .await
            .unwrap_or(uploads_dir.clone()),
    ) {
        let relative_to_uploads = canonical
            .strip_prefix(
                tokio::fs::canonicalize(&uploads_dir)
                    .await
                    .unwrap_or(uploads_dir),
            )
            .map_err(|_| ToolError::ExecutionFailed("path error".into()))?;
        let chrome_path = format!("{CHROME_UPLOADS_MOUNT}/{}", relative_to_uploads.display());
        return Ok((chrome_path, None));
    }

    // Otherwise, copy to a staging area Chrome can access.
    let staging_dir = uploads_dir.join(".browser-staging");
    tokio::fs::create_dir_all(&staging_dir)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to create staging dir: {e}")))?;

    let filename = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let staged_name = format!("{}_{filename}", ulid::Ulid::new());
    let staged_path = staging_dir.join(&staged_name);

    tokio::fs::copy(&canonical, &staged_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to stage file: {e}")))?;

    let chrome_path = format!("{CHROME_UPLOADS_MOUNT}/.browser-staging/{staged_name}");
    Ok((chrome_path, Some(staged_path)))
}

async fn execute_upload(
    mgr: &mut BrowserManager,
    params: &Value,
    ctx: &ToolContext,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'upload' requires 'ref' parameter".into()))?;
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'upload' requires 'path' parameter".into()))?;

    let (chrome_path, staging) = stage_for_chrome(&ctx.workspace, path).await?;

    let result = mgr
        .upload(ref_id, &[chrome_path])
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()));

    // Clean up staging copy regardless of success/failure.
    if let Some(staged) = staging {
        let _ = tokio::fs::remove_file(&staged).await;
    }

    let desc = result?;
    let url = mgr.current_url().await.unwrap_or_default();
    let output = json!({
        "ok": true,
        "url": url,
        "description": desc,
        "path": path,
    });
    Ok(ToolOutput::text(output.to_string()))
}

async fn execute_tabs(mgr: &mut BrowserManager) -> Result<ToolOutput, ToolError> {
    let tabs = mgr
        .list_tabs()
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let active_id = mgr.active_tab_id();
    let mut lines = vec!["Open tabs:".to_string()];
    for tab in &tabs {
        let marker = if Some(tab.id) == active_id {
            " [active]"
        } else {
            ""
        };
        lines.push(format!(
            "  Tab {}: {} — {}{}",
            tab.id, tab.title, tab.url, marker
        ));
    }
    Ok(ToolOutput::text(lines.join("\n")))
}

async fn execute_open(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let url = params.get("url").and_then(Value::as_str);
    let xml = mgr
        .open_tab(url)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url_str = mgr.current_url().await.unwrap_or_default();
    let wrapped = format!(
        "<<<EXTERNAL_UNTRUSTED_CONTENT>>>\n\
         Source: Browser ({url_str})\n\
         ---\n\
         {xml}\n\
         <<<END_EXTERNAL_UNTRUSTED_CONTENT>>>"
    );
    Ok(ToolOutput::text(wrapped))
}

async fn execute_focus(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let tab_id = params
        .get("tab")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'focus' requires 'tab' parameter".into()))?
        as u32;
    let xml = mgr
        .focus_tab(tab_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = mgr.current_url().await.unwrap_or_default();
    let wrapped = format!(
        "<<<EXTERNAL_UNTRUSTED_CONTENT>>>\n\
         Source: Browser ({url})\n\
         ---\n\
         {xml}\n\
         <<<END_EXTERNAL_UNTRUSTED_CONTENT>>>"
    );
    Ok(ToolOutput::text(wrapped))
}

async fn execute_close(mgr: &mut BrowserManager, params: &Value) -> Result<ToolOutput, ToolError> {
    let tab_id = params
        .get("tab")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::InvalidParams("'close' requires 'tab' parameter".into()))?
        as u32;
    let msg = mgr
        .close_tab(tab_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    Ok(ToolOutput::text(
        json!({"ok": true, "description": msg}).to_string(),
    ))
}

async fn execute_browsers(mgr: &mut BrowserManager) -> Result<ToolOutput, ToolError> {
    let browsers = mgr.list_browsers();
    let active = mgr.active_browser_name();
    if browsers.is_empty() {
        return Ok(ToolOutput::text("No browsers registered.".to_string()));
    }
    let mut lines = vec!["Known browsers:".to_string()];
    // Sort by name for deterministic output.
    let mut sorted = browsers;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for b in &sorted {
        let active_marker = if active == Some(b.name.as_str()) {
            " [active]"
        } else {
            ""
        };
        let status = if b.connected {
            "connected"
        } else {
            "disconnected"
        };
        let origin = if b.discovered { " (discovered)" } else { "" };
        lines.push(format!(
            "  {}{}: {} — {} tab(s), {}{}",
            b.name, active_marker, b.cdp_url, b.tab_count, status, origin,
        ));
    }
    Ok(ToolOutput::text(lines.join("\n")))
}

async fn execute_connect(
    mgr: &mut BrowserManager,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'connect' requires 'name' parameter".into()))?;
    let cdp_url = params
        .get("cdp_url")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'connect' requires 'cdp_url' parameter".into()))?;
    let info = mgr
        .connect_browser(name, cdp_url)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let result = json!({
        "ok": true,
        "name": info.name,
        "cdp_url": info.cdp_url,
        "connected": info.connected,
        "tab_count": info.tab_count,
        "active": true,
    });
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_disconnect(
    mgr: &mut BrowserManager,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'disconnect' requires 'name' parameter".into()))?;
    mgr.disconnect_browser(name)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    Ok(ToolOutput::text(
        json!({"ok": true, "description": format!("Disconnected browser '{name}'")}).to_string(),
    ))
}

async fn execute_discover() -> Result<ToolOutput, ToolError> {
    let found = crate::web::browser::discovery::discover().await;
    if found.is_empty() {
        return Ok(ToolOutput::text("No CDP endpoints found.".to_string()));
    }
    let mut lines = vec!["Discovered CDP endpoints:".to_string()];
    for b in &found {
        let version = b.browser_version.as_deref().unwrap_or("unknown");
        lines.push(format!(
            "  {}:{} — {} ({})",
            b.host, b.port, version, b.cdp_url
        ));
    }
    Ok(ToolOutput::text(lines.join("\n")))
}
