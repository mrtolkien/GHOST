use chromiumoxide::cdp::browser_protocol::{
    accessibility::EnableParams as AxEnableParams,
    dom::{
        BackendNodeId, FocusParams, GetBoxModelParams, ResolveNodeParams,
        ScrollIntoViewIfNeededParams, SetFileInputFilesParams,
    },
    emulation::SetDeviceMetricsOverrideParams,
    input::{
        DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams,
        DispatchMouseEventType, InsertTextParams, MouseButton,
    },
    page::CaptureScreenshotFormat,
};
use chromiumoxide::cdp::js_protocol::runtime::{CallArgument, CallFunctionOnParams};
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::{Browser, Page};
use futures::StreamExt;
use tokio::task::JoinHandle;
use tracing::debug;

use super::error::BrowserError;

pub const NAVIGATION_TIMEOUT_SECS: u64 = 30;
pub const VIEWPORT_WIDTH: u32 = 1280;
pub const VIEWPORT_HEIGHT: u32 = 720;

/// Page scroll amount in CSS pixels per scroll action.
const SCROLL_DELTA_PX: i32 = 600;

#[derive(Debug, Clone, Copy)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Connect to a running Chrome instance via its CDP WebSocket URL.
///
/// Accepts either a full WebSocket URL (e.g.
/// `ws://localhost:9222/devtools/browser/...`) or a base URL (e.g.
/// `ws://localhost:9222`). When a base URL is given, queries
/// `/json/version` to discover the `webSocketDebuggerUrl`.
///
/// Returns the browser handle and a spawned handler task that must remain
/// alive for the connection to work.
pub async fn connect(cdp_url: &str) -> Result<(Browser, JoinHandle<()>), BrowserError> {
    let ws_url = resolve_ws_url(cdp_url).await?;

    let (browser, mut handler) =
        Browser::connect(&ws_url)
            .await
            .map_err(|e| BrowserError::ConnectionFailed {
                url: ws_url.clone(),
                source: Box::new(e),
            })?;

    let handle = tokio::spawn(async move { while handler.next().await.is_some() {} });

    debug!(url = ws_url, "connected to Chrome via CDP");
    Ok((browser, handle))
}

/// If `cdp_url` is a base URL (no path or just `/`), query Chrome's
/// `/json/version` endpoint to discover the actual WebSocket URL.
async fn resolve_ws_url(cdp_url: &str) -> Result<String, BrowserError> {
    let parsed = url::Url::parse(cdp_url).map_err(|e| BrowserError::ConnectionFailed {
        url: cdp_url.to_owned(),
        source: Box::new(e),
    })?;

    // If the URL has a non-trivial path, assume it's already a full WS URL.
    let path = parsed.path();
    if !path.is_empty() && path != "/" {
        return Ok(cdp_url.to_string());
    }

    // Query /json/version for the webSocketDebuggerUrl
    let http_url = format!(
        "http://{}:{}/json/version",
        parsed.host_str().unwrap_or("localhost"),
        parsed.port().unwrap_or(9222)
    );
    let resp: serde_json::Value = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("reqwest client")
        .get(&http_url)
        .send()
        .await
        .map_err(|e| BrowserError::ConnectionFailed {
            url: cdp_url.to_owned(),
            source: Box::new(e),
        })?
        .json()
        .await
        .map_err(|e| BrowserError::ConnectionFailed {
            url: cdp_url.to_owned(),
            source: Box::new(e),
        })?;

    resp.get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| BrowserError::ConnectionFailed {
            url: cdp_url.to_owned(),
            source: "no webSocketDebuggerUrl in /json/version response".into(),
        })
}

/// Open a new blank tab and configure the viewport.
pub async fn new_page(browser: &Browser) -> Result<Page, BrowserError> {
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("failed to open new page: {e}"),
        })?;

    // Set viewport size via Emulation.setDeviceMetricsOverride.
    page.execute(SetDeviceMetricsOverrideParams::new(
        VIEWPORT_WIDTH as i64,
        VIEWPORT_HEIGHT as i64,
        1.0,   // device_scale_factor
        false, // mobile
    ))
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("failed to set viewport: {e}"),
    })?;

    Ok(page)
}

/// Navigate to `url`, wait for load, return `(final_url, title)`.
pub async fn navigate(page: &Page, url: &str) -> Result<(String, String), BrowserError> {
    let nav = tokio::time::timeout(
        std::time::Duration::from_secs(NAVIGATION_TIMEOUT_SECS),
        page.goto(url),
    )
    .await
    .map_err(|_| BrowserError::NavigationTimeout {
        url: url.to_owned(),
        timeout_secs: NAVIGATION_TIMEOUT_SECS,
    })?
    .map_err(|e| BrowserError::NavigationFailed {
        url: url.to_owned(),
        reason: e.to_string(),
    })?;

    // Retrieve the final URL (may differ from input after redirects).
    let final_url = nav
        .url()
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("failed to get page URL: {e}"),
        })?
        .unwrap_or_else(|| url.to_owned());

    // Retrieve the page title via JS evaluation.
    let title: String = page
        .evaluate("document.title")
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("failed to evaluate document.title: {e}"),
        })?
        .into_value()
        .unwrap_or_default();

    // Brief pause to ensure the accessibility tree is populated. Chrome's
    // load event fires before the AX tree is fully built for some pages.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    debug!(url = %final_url, title = %title, "navigation complete");
    Ok((final_url, title))
}

/// Raw CDP command for `Accessibility.getFullAXTree` that returns JSON
/// instead of typed `AxNode`. This avoids deserialization failures when Chrome
/// includes `AxPropertyName` variants (e.g. "uninteresting") that
/// chromiumoxide_cdp 0.7.0 doesn't recognize.
#[derive(Debug, serde::Serialize)]
struct RawGetFullAxTree {}

/// Raw response for `Accessibility.getFullAXTree`.
#[derive(Debug, serde::Deserialize)]
struct RawAxTreeResponse {
    nodes: Vec<serde_json::Value>,
}

impl chromiumoxide::types::Method for RawGetFullAxTree {
    fn identifier(&self) -> chromiumoxide::types::MethodId {
        "Accessibility.getFullAXTree".into()
    }
}

impl chromiumoxide::Command for RawGetFullAxTree {
    type Response = RawAxTreeResponse;
}

/// Fetch the full accessibility tree for the current page as raw JSON nodes.
///
/// Returns `Vec<serde_json::Value>` instead of typed `CdpAxNode` because
/// chromiumoxide_cdp 0.7.0 lacks some `AxPropertyName` variants (e.g.
/// "uninteresting") that Chrome 146+ includes in `ignoredReasons`, causing
/// deserialization failures. Our `parse_ax_tree` handles the raw JSON.
pub async fn get_accessibility_tree(page: &Page) -> Result<Vec<serde_json::Value>, BrowserError> {
    // Enable the accessibility domain.
    page.execute(AxEnableParams::default())
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("failed to enable accessibility: {e}"),
        })?;

    let resp = page
        .execute(RawGetFullAxTree {})
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("failed to get accessibility tree: {e}"),
        })?;

    debug!(node_count = resp.result.nodes.len(), "AX tree fetched");

    Ok(resp.result.nodes)
}

/// Click an element identified by its `BackendNodeId`.
///
/// Retrieves the element's box model to find its center, then dispatches
/// `mousePressed` + `mouseReleased` events at those coordinates.
pub async fn click_node(page: &Page, backend_node_id: i64) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);

    // Scroll the element into view first so the box model is in-viewport.
    page.execute(
        ScrollIntoViewIfNeededParams::builder()
            .backend_node_id(node_id)
            .build(),
    )
    .await
    .map_err(|e| BrowserError::NotInteractable {
        ref_id: format!("backend:{backend_node_id}"),
        reason: format!("scroll into view failed: {e}"),
    })?;

    // Get the content box to calculate click coordinates.
    let box_model = page
        .execute(
            GetBoxModelParams::builder()
                .backend_node_id(node_id)
                .build(),
        )
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("could not compute box model: {e}"),
        })?;

    let (cx, cy) = quad_center(box_model.result.model.content.inner());

    // mousePressed
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MousePressed)
            .x(cx)
            .y(cy)
            .button(MouseButton::Left)
            .click_count(1)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mousePressed dispatch failed: {e}"),
    })?;

    // mouseReleased
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseReleased)
            .x(cx)
            .y(cy)
            .button(MouseButton::Left)
            .click_count(1)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mouseReleased dispatch failed: {e}"),
    })?;

    debug!(backend_node_id, x = cx, y = cy, "clicked element");
    Ok(())
}

/// Type text into an element identified by its `BackendNodeId`.
///
/// Focuses the element via `DOM.focus`, then uses `Input.insertText`.
pub async fn type_into_node(
    page: &Page,
    backend_node_id: i64,
    text: &str,
) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);

    // Focus the target element.
    page.execute(FocusParams::builder().backend_node_id(node_id).build())
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("focus failed: {e}"),
        })?;

    // Insert text (works like paste — no key events, just the final string).
    page.execute(InsertTextParams::new(text))
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("insertText failed: {e}"),
        })?;

    debug!(backend_node_id, chars = text.len(), "typed into element");
    Ok(())
}

/// Scroll the page or a specific element into view.
///
/// If `backend_node_id` is `Some`, scrolls that element into the viewport
/// using `DOM.scrollIntoViewIfNeeded`. Otherwise scrolls the page by
/// `SCROLL_DELTA_PX` in the given direction.
pub async fn scroll(
    page: &Page,
    direction: ScrollDirection,
    backend_node_id: Option<i64>,
) -> Result<(), BrowserError> {
    if let Some(id) = backend_node_id {
        page.execute(
            ScrollIntoViewIfNeededParams::builder()
                .backend_node_id(BackendNodeId::new(id))
                .build(),
        )
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{id}"),
            reason: format!("scrollIntoView failed: {e}"),
        })?;
    } else {
        let delta = match direction {
            ScrollDirection::Down => SCROLL_DELTA_PX,
            ScrollDirection::Up => -SCROLL_DELTA_PX,
        };
        let expr = format!("window.scrollBy(0, {delta})");
        page.evaluate(expr)
            .await
            .map_err(|e| BrowserError::CdpError {
                message: format!("page scroll failed: {e}"),
            })?;
    }
    Ok(())
}

/// Capture a PNG screenshot of the current viewport.
pub async fn screenshot(page: &Page) -> Result<Vec<u8>, BrowserError> {
    let params = ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Png)
        .build();

    page.screenshot(params)
        .await
        .map_err(|e| BrowserError::ScreenshotFailed {
            reason: e.to_string(),
        })
}

/// Send a keyboard key press (keyDown + keyUp).
///
/// The `key` parameter is a DOM key name (e.g. "Enter", "Escape", "Tab",
/// "ArrowDown"). For Enter, sets `text: "\r"`. For single printable
/// characters, sets `text` to the character itself.
pub async fn press_key(page: &Page, key: &str) -> Result<(), BrowserError> {
    // Determine the text payload for the key event.
    let text = match key {
        "Enter" => Some("\r".to_string()),
        k if k.chars().count() == 1 => Some(k.to_string()),
        _ => None,
    };

    let mut down = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyDown)
        .key(key);
    if let Some(ref t) = text {
        down = down.text(t.clone());
    }
    page.execute(
        down.build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("keyDown dispatch failed: {e}"),
    })?;

    let up = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyUp)
        .key(key)
        .build()
        .map_err(|e| BrowserError::CdpError { message: e })?;
    page.execute(up).await.map_err(|e| BrowserError::CdpError {
        message: format!("keyUp dispatch failed: {e}"),
    })?;

    debug!(key, "pressed key");
    Ok(())
}

/// Hover over an element identified by its `BackendNodeId`.
///
/// Scrolls the element into view, gets the box model center, then
/// dispatches a `mouseMoved` event at those coordinates.
pub async fn hover_node(page: &Page, backend_node_id: i64) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);

    page.execute(
        ScrollIntoViewIfNeededParams::builder()
            .backend_node_id(node_id)
            .build(),
    )
    .await
    .map_err(|e| BrowserError::NotInteractable {
        ref_id: format!("backend:{backend_node_id}"),
        reason: format!("scroll into view failed: {e}"),
    })?;

    let box_model = page
        .execute(
            GetBoxModelParams::builder()
                .backend_node_id(node_id)
                .build(),
        )
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("could not compute box model: {e}"),
        })?;

    let (cx, cy) = quad_center(box_model.result.model.content.inner());

    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseMoved)
            .x(cx)
            .y(cy)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mouseMoved dispatch failed: {e}"),
    })?;

    debug!(backend_node_id, x = cx, y = cy, "hovered element");
    Ok(())
}

/// Select an option value in a `<select>` element.
///
/// Resolves the backend node to a JS object, then calls a function on it
/// to set the value and dispatch a `change` event.
pub async fn select_option(
    page: &Page,
    backend_node_id: i64,
    value: &str,
) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);

    // Resolve the DOM node to a Runtime.RemoteObject.
    let resolved = page
        .execute(
            ResolveNodeParams::builder()
                .backend_node_id(node_id)
                .build(),
        )
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("resolveNode failed: {e}"),
        })?;

    let object_id =
        resolved
            .result
            .object
            .object_id
            .ok_or_else(|| BrowserError::NotInteractable {
                ref_id: format!("backend:{backend_node_id}"),
                reason: "resolveNode returned no objectId".to_string(),
            })?;

    // Call a function on the element to set the value and fire change.
    let js = "function(v) { \
        this.value = v; \
        this.dispatchEvent(new Event('change', {bubbles: true})); \
    }";
    let arg = CallArgument::builder()
        .value(serde_json::Value::String(value.to_string()))
        .build();

    page.execute(
        CallFunctionOnParams::builder()
            .function_declaration(js)
            .object_id(object_id)
            .argument(arg)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("callFunctionOn failed: {e}"),
    })?;

    debug!(backend_node_id, value, "selected option");
    Ok(())
}

/// Fill a form field by clearing existing content then inserting new text.
///
/// Focuses the element, selects all existing content (Ctrl+A), then
/// inserts the new text, replacing whatever was there.
pub async fn fill_field(
    page: &Page,
    backend_node_id: i64,
    value: &str,
) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);

    // Focus the target element.
    page.execute(FocusParams::builder().backend_node_id(node_id).build())
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("focus failed: {e}"),
        })?;

    // Select all existing content with Ctrl+A.
    page.execute(
        DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::KeyDown)
            .key("a")
            .modifiers(2_i64) // Ctrl
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("select-all keyDown failed: {e}"),
    })?;

    page.execute(
        DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::KeyUp)
            .key("a")
            .modifiers(2_i64)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("select-all keyUp failed: {e}"),
    })?;

    // Insert the new text (replaces the selection).
    page.execute(InsertTextParams::new(value))
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{backend_node_id}"),
            reason: format!("insertText failed: {e}"),
        })?;

    debug!(backend_node_id, chars = value.len(), "filled field");
    Ok(())
}

/// Wait for an element to be resolvable via `DOM.resolveNode`, polling
/// until success or timeout.
///
/// If `backend_node_id` is provided, polls until the node can be resolved.
/// The caller is responsible for sleeping when no node ID is given.
pub async fn wait_for_element(
    page: &Page,
    backend_node_id: i64,
    timeout_ms: u64,
) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        let result = page
            .execute(
                ResolveNodeParams::builder()
                    .backend_node_id(node_id)
                    .build(),
            )
            .await;

        if result.is_ok() {
            debug!(backend_node_id, "element found");
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(BrowserError::CdpError {
                message: format!(
                    "timed out waiting for element backend:{backend_node_id} \
                     after {timeout_ms}ms"
                ),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Evaluate a JavaScript expression and return the string result.
pub async fn evaluate_js(page: &Page, expression: &str) -> Result<String, BrowserError> {
    let result = page
        .evaluate(expression)
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("JS evaluation failed: {e}"),
        })?;

    // Try to extract a string value, falling back to the JSON
    // representation.
    let value: String = result.into_value().unwrap_or_else(|v| format!("{v:?}"));

    debug!(
        expr_len = expression.len(),
        result_len = value.len(),
        "evaluated JS"
    );
    Ok(value)
}

/// Drag from one element to another by dispatching mouse events.
///
/// Dispatches mouseMoved → mousePressed at the source center, then
/// mouseMoved → mouseReleased at the target center.
pub async fn drag_node(
    page: &Page,
    from_node_id: i64,
    to_node_id: i64,
) -> Result<(), BrowserError> {
    // Scroll source into view and get its center.
    let from_id = BackendNodeId::new(from_node_id);
    page.execute(
        ScrollIntoViewIfNeededParams::builder()
            .backend_node_id(from_id)
            .build(),
    )
    .await
    .map_err(|e| BrowserError::NotInteractable {
        ref_id: format!("backend:{from_node_id}"),
        reason: format!("scroll into view failed: {e}"),
    })?;

    let from_box = page
        .execute(
            GetBoxModelParams::builder()
                .backend_node_id(from_id)
                .build(),
        )
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{from_node_id}"),
            reason: format!("could not compute box model: {e}"),
        })?;
    let (sx, sy) = quad_center(from_box.result.model.content.inner());

    // Scroll target into view and get its center.
    let to_id = BackendNodeId::new(to_node_id);
    page.execute(
        ScrollIntoViewIfNeededParams::builder()
            .backend_node_id(to_id)
            .build(),
    )
    .await
    .map_err(|e| BrowserError::NotInteractable {
        ref_id: format!("backend:{to_node_id}"),
        reason: format!("scroll into view failed: {e}"),
    })?;

    let to_box = page
        .execute(GetBoxModelParams::builder().backend_node_id(to_id).build())
        .await
        .map_err(|e| BrowserError::NotInteractable {
            ref_id: format!("backend:{to_node_id}"),
            reason: format!("could not compute box model: {e}"),
        })?;
    let (tx, ty) = quad_center(to_box.result.model.content.inner());

    // 1. mouseMoved to source
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseMoved)
            .x(sx)
            .y(sy)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mouseMoved to source failed: {e}"),
    })?;

    // 2. mousePressed at source
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MousePressed)
            .x(sx)
            .y(sy)
            .button(MouseButton::Left)
            .click_count(1)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mousePressed at source failed: {e}"),
    })?;

    // 3. mouseMoved to target (with button held)
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseMoved)
            .x(tx)
            .y(ty)
            .button(MouseButton::Left)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mouseMoved to target failed: {e}"),
    })?;

    // 4. mouseReleased at target
    page.execute(
        DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseReleased)
            .x(tx)
            .y(ty)
            .button(MouseButton::Left)
            .click_count(1)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("mouseReleased at target failed: {e}"),
    })?;

    debug!(from = from_node_id, to = to_node_id, "dragged element");
    Ok(())
}

/// Resize the browser viewport via `Emulation.setDeviceMetricsOverride`.
pub async fn resize_viewport(page: &Page, width: u32, height: u32) -> Result<(), BrowserError> {
    page.execute(SetDeviceMetricsOverrideParams::new(
        width as i64,
        height as i64,
        1.0,   // device_scale_factor
        false, // mobile
    ))
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("failed to resize viewport: {e}"),
    })?;

    debug!(width, height, "viewport resized");
    Ok(())
}

/// Set file paths on a `<input type="file">` element via
/// `DOM.setFileInputFiles`.
///
/// `files` are paths as seen by the Chrome process (e.g. `/uploads/data.csv`
/// inside the container).
pub async fn set_file_input_files(
    page: &Page,
    backend_node_id: i64,
    files: &[String],
) -> Result<(), BrowserError> {
    let node_id = BackendNodeId::new(backend_node_id);

    page.execute(
        SetFileInputFilesParams::builder()
            .files(files.iter().cloned())
            .backend_node_id(node_id)
            .build()
            .map_err(|e| BrowserError::CdpError { message: e })?,
    )
    .await
    .map_err(|e| BrowserError::CdpError {
        message: format!("setFileInputFiles failed: {e}"),
    })?;

    debug!(
        backend_node_id,
        file_count = files.len(),
        "set file input files"
    );
    Ok(())
}

/// Calculate the center point of a CDP Quad (array of 8 floats:
/// x0,y0, x1,y1, x2,y2, x3,y3).
fn quad_center(points: &[f64]) -> (f64, f64) {
    if points.len() < 8 {
        return (0.0, 0.0);
    }
    let cx = (points[0] + points[2] + points[4] + points[6]) / 4.0;
    let cy = (points[1] + points[3] + points[5] + points[7]) / 4.0;
    (cx, cy)
}
