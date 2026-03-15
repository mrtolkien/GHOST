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
                reads."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "navigate", "snapshot", "click",
                            "type", "scroll", "screenshot"
                        ],
                        "description": "navigate: open URL (replaces current page). \
                            snapshot: get accessibility tree with ref IDs. \
                            click: click element by ref. \
                            type: enter text into element by ref. \
                            scroll: scroll page or element. \
                            screenshot: capture page as PNG image."
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to. Required for 'navigate'."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Element ref ID (e.g. 'e5'). Required for \
                            'click' and 'type'. Optional for 'scroll' (scrolls \
                            element into view)."
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
                    }
                },
                "required": ["action"]
            }),
        }
    }

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
        let session = guard.as_mut().unwrap();

        match action {
            "navigate" => execute_navigate(session, &params).await,
            "snapshot" => execute_snapshot(session, &params).await,
            "click" => execute_click(session, &params).await,
            "type" => execute_type(session, &params).await,
            "scroll" => execute_scroll(session, &params).await,
            "screenshot" => execute_screenshot(session, &params, ctx).await,
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
        "description": "Screenshot captured (1280x720)"
    });
    Ok(ToolOutput::text(result.to_string()))
}
