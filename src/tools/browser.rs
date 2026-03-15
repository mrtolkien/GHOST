use async_trait::async_trait;
use serde_json::{Value, json};

use crate::providers::ToolDefinition;
use crate::web::browser::cdp::ScrollDirection;

use super::context::ToolContext;
use super::error::ToolError;
use super::manager::Tool;
use super::output::ToolOutput;

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
                            "drag", "resize"
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
                            resize: resize the browser viewport."
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

        // Lazy-init: lock the Arc<Mutex<Option<BrowserSession>>>, create
        // if None
        let mut guard = ctx.browser_session.lock().await;
        if guard.is_none() {
            let cdp_url = ctx.config.web.chrome_cdp_url.as_deref().ok_or_else(|| {
                ToolError::ExecutionFailed("chrome_cdp_url not configured".into())
            })?;
            let session = crate::web::browser::BrowserSession::connect(cdp_url)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            *guard = Some(session);
        }
        let session = guard
            .as_mut()
            .ok_or_else(|| ToolError::ExecutionFailed("browser session unavailable".into()))?;

        match action {
            "navigate" => execute_navigate(session, &params).await,
            "snapshot" => execute_snapshot(session, &params).await,
            "click" => execute_click(session, &params).await,
            "type" => execute_type(session, &params).await,
            "scroll" => execute_scroll(session, &params).await,
            "screenshot" => execute_screenshot(session, &params, ctx).await,
            "press" => execute_press(session, &params).await,
            "hover" => execute_hover(session, &params).await,
            "select" => execute_select(session, &params).await,
            "fill" => execute_fill(session, &params).await,
            "wait" => execute_wait(session, &params).await,
            "evaluate" => execute_evaluate(session, &params).await,
            "drag" => execute_drag(session, &params).await,
            "resize" => execute_resize(session, &params).await,
            _ => Err(ToolError::InvalidParams(format!(
                "unknown action: {action}"
            ))),
        }
    }
}

async fn execute_navigate(
    session: &mut crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let url = params
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'navigate' requires 'url' parameter".into()))?;
    let (final_url, title) = session
        .navigate(url)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let result = json!({"ok": true, "url": final_url, "title": title});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_snapshot(
    session: &mut crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let xml = session
        .snapshot(offset)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let wrapped = format!(
        "<<<EXTERNAL_UNTRUSTED_CONTENT>>>\n\
         Source: Browser ({url})\n\
         ---\n\
         {xml}\n\
         <<<END_EXTERNAL_UNTRUSTED_CONTENT>>>"
    );
    Ok(ToolOutput::text(wrapped))
}

async fn execute_click(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'click' requires 'ref' parameter".into()))?;
    let desc = session
        .click(ref_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_type(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'type' requires 'ref' parameter".into()))?;
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'type' requires 'text' parameter".into()))?;
    let desc = session
        .type_text(ref_id, text)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_scroll(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let direction = match params.get("direction").and_then(Value::as_str) {
        Some("up") => ScrollDirection::Up,
        _ => ScrollDirection::Down,
    };
    let ref_id = params.get("ref").and_then(Value::as_str);
    let desc = session
        .scroll(direction, ref_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_screenshot(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
    ctx: &ToolContext,
) -> Result<ToolOutput, ToolError> {
    let _ = params; // unused but kept for signature consistency
    let path = session
        .screenshot(&ctx.workspace)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let rel = path.strip_prefix(&ctx.workspace).unwrap_or(&path);
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({
        "ok": true,
        "url": url,
        "path": rel.display().to_string(),
        "description": format!("Screenshot captured ({}x{})", session.viewport_size().0, session.viewport_size().1)
    });
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_press(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let key = params
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'press' requires 'key' parameter".into()))?;
    let desc = session
        .press(key)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_hover(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'hover' requires 'ref' parameter".into()))?;
    let desc = session
        .hover(ref_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_select(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'select' requires 'ref' parameter".into()))?;
    let value = params
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'select' requires 'value' parameter".into()))?;
    let desc = session
        .select(ref_id, value)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_fill(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
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

    let desc = session
        .fill(&fields)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_wait(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params.get("ref").and_then(Value::as_str);
    let timeout_ms = params
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(1000);
    let desc = session
        .wait(ref_id, timeout_ms)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_evaluate(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let expression = params
        .get("expression")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolError::InvalidParams("'evaluate' requires 'expression' parameter".into())
        })?;
    let result_str = session
        .evaluate(expression)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
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

async fn execute_drag(
    session: &crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
    let ref_id = params
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'drag' requires 'ref' parameter".into()))?;
    let target_ref = params
        .get("target_ref")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams("'drag' requires 'target_ref' parameter".into()))?;
    let desc = session
        .drag(ref_id, target_ref)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}

async fn execute_resize(
    session: &mut crate::web::browser::BrowserSession,
    params: &Value,
) -> Result<ToolOutput, ToolError> {
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
    let desc = session
        .resize(width, height)
        .await
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
    let url = session.current_url().await.unwrap_or_default();
    let result = json!({"ok": true, "url": url, "description": desc});
    Ok(ToolOutput::text(result.to_string()))
}
