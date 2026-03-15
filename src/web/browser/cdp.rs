use chromiumoxide::cdp::browser_protocol::{
    accessibility::{AxNode as CdpAxNode, GetFullAxTreeParams},
    dom::{BackendNodeId, FocusParams, GetBoxModelParams, ScrollIntoViewIfNeededParams},
    emulation::SetDeviceMetricsOverrideParams,
    input::{DispatchMouseEventParams, DispatchMouseEventType, InsertTextParams, MouseButton},
    page::CaptureScreenshotFormat,
};
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
/// Returns the browser handle and a spawned handler task that must remain
/// alive for the connection to work.
pub async fn connect(cdp_url: &str) -> Result<(Browser, JoinHandle<()>), BrowserError> {
    let (browser, mut handler) =
        Browser::connect(cdp_url)
            .await
            .map_err(|e| BrowserError::ConnectionFailed {
                url: cdp_url.to_owned(),
                source: Box::new(e),
            })?;

    let handle = tokio::spawn(async move { while handler.next().await.is_some() {} });

    debug!(url = cdp_url, "connected to Chrome via CDP");
    Ok((browser, handle))
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

    debug!(url = %final_url, title = %title, "navigation complete");
    Ok((final_url, title))
}

/// Fetch the full accessibility tree for the current page.
pub async fn get_accessibility_tree(page: &Page) -> Result<Vec<CdpAxNode>, BrowserError> {
    let resp = page
        .execute(GetFullAxTreeParams::default())
        .await
        .map_err(|e| BrowserError::CdpError {
            message: format!("failed to get accessibility tree: {e}"),
        })?;

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
