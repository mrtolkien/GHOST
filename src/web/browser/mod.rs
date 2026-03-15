pub mod accessibility;
pub mod cdp;
pub mod error;
pub mod url_check;

use std::path::{Path, PathBuf};

use tokio::task::JoinHandle;

use self::accessibility::{RefMap, parse_ax_tree, render_xml};
pub use self::cdp::ScrollDirection;
pub use self::error::BrowserError;

const MAX_SNAPSHOT_NODES: usize = 500;
const MAX_SNAPSHOT_DEPTH: usize = 15;

/// A browser session connected to Chrome via CDP.
///
/// Manages a single tab, maintains ref map between snapshots.
/// Created lazily on first browser tool call, dropped on session end.
pub struct BrowserSession {
    page: chromiumoxide::Page,
    refs: RefMap,
    _browser: chromiumoxide::Browser,
    _handler: JoinHandle<()>,
    viewport_width: u32,
    viewport_height: u32,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self._handler.abort();
    }
}

impl BrowserSession {
    pub async fn connect(cdp_url: &str) -> Result<Self, BrowserError> {
        let (browser, handler) = cdp::connect(cdp_url).await?;
        let page = cdp::new_page(&browser).await?;
        Ok(Self {
            page,
            refs: RefMap::new(),
            _browser: browser,
            _handler: handler,
            viewport_width: cdp::VIEWPORT_WIDTH,
            viewport_height: cdp::VIEWPORT_HEIGHT,
        })
    }

    pub async fn navigate(&mut self, url: &str) -> Result<(String, String), BrowserError> {
        url_check::validate_url(url)?;
        self.refs.reset();
        cdp::navigate(&self.page, url).await
    }

    pub async fn snapshot(&mut self, offset: usize) -> Result<String, BrowserError> {
        self.refs.reset();
        let raw_nodes = cdp::get_accessibility_tree(&self.page).await?;
        tracing::debug!(node_count = raw_nodes.len(), "raw AX tree fetched");
        let tree = parse_ax_tree(&raw_nodes);
        tracing::debug!(tree_size = tree.len(), "parsed AX tree");
        let xml = render_xml(
            &tree,
            &mut self.refs,
            MAX_SNAPSHOT_NODES,
            MAX_SNAPSHOT_DEPTH,
            offset,
        );
        Ok(xml)
    }

    pub async fn click(&self, ref_id: &str) -> Result<String, BrowserError> {
        let node_id = self
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.to_string(),
            })?;
        cdp::click_node(&self.page, node_id).await?;
        Ok(format!("Clicked [ref={ref_id}]"))
    }

    pub async fn type_text(&self, ref_id: &str, text: &str) -> Result<String, BrowserError> {
        let node_id = self
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.to_string(),
            })?;
        cdp::type_into_node(&self.page, node_id, text).await?;
        Ok(format!("Typed into [ref={ref_id}]"))
    }

    pub async fn scroll(
        &self,
        direction: ScrollDirection,
        ref_id: Option<&str>,
    ) -> Result<String, BrowserError> {
        let node_id = ref_id
            .map(|r| {
                self.refs
                    .resolve(r)
                    .ok_or_else(|| BrowserError::RefNotFound {
                        ref_id: r.to_string(),
                    })
            })
            .transpose()?;
        cdp::scroll(&self.page, direction, node_id).await?;
        let dir = match direction {
            ScrollDirection::Up => "up",
            ScrollDirection::Down => "down",
        };
        Ok(format!("Scrolled {dir}"))
    }

    pub async fn screenshot(&self, workspace: &Path) -> Result<PathBuf, BrowserError> {
        let bytes = cdp::screenshot(&self.page).await?;
        let dir = workspace.join(".cache/browser");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| BrowserError::ScreenshotFailed {
                reason: e.to_string(),
            })?;
        let filename = format!(
            "screenshot-{}.png",
            chrono::Utc::now().format("%Y-%m-%d-%H%M%S")
        );
        let path = dir.join(&filename);
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| BrowserError::ScreenshotFailed {
                reason: e.to_string(),
            })?;
        Ok(path)
    }

    /// Send a keyboard key press.
    pub async fn press(&self, key: &str) -> Result<String, BrowserError> {
        cdp::press_key(&self.page, key).await?;
        Ok(format!("Pressed {key}"))
    }

    /// Hover over an element by ref ID.
    pub async fn hover(&self, ref_id: &str) -> Result<String, BrowserError> {
        let node_id = self
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.to_string(),
            })?;
        cdp::hover_node(&self.page, node_id).await?;
        Ok(format!("Hovered [ref={ref_id}]"))
    }

    /// Select an option value in a `<select>` element by ref ID.
    pub async fn select(&self, ref_id: &str, value: &str) -> Result<String, BrowserError> {
        let node_id = self
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.to_string(),
            })?;
        cdp::select_option(&self.page, node_id, value).await?;
        Ok(format!("Selected '{value}' in [ref={ref_id}]"))
    }

    /// Fill multiple form fields, clearing each before typing.
    ///
    /// Each pair is `(ref_id, value)`.
    pub async fn fill(&self, fields: &[(String, String)]) -> Result<String, BrowserError> {
        for (ref_id, value) in fields {
            let node_id = self
                .refs
                .resolve(ref_id)
                .ok_or_else(|| BrowserError::RefNotFound {
                    ref_id: ref_id.to_string(),
                })?;
            cdp::fill_field(&self.page, node_id, value).await?;
        }
        Ok(format!("Filled {} fields", fields.len()))
    }

    /// Wait for an element or a fixed duration.
    ///
    /// If `ref_id` is provided, polls until the element is resolvable.
    /// Otherwise sleeps for `timeout_ms`.
    pub async fn wait(
        &self,
        ref_id: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String, BrowserError> {
        if let Some(r) = ref_id {
            let node_id = self
                .refs
                .resolve(r)
                .ok_or_else(|| BrowserError::RefNotFound {
                    ref_id: r.to_string(),
                })?;
            cdp::wait_for_element(&self.page, node_id, timeout_ms).await?;
            Ok(format!("Waited for [ref={r}]"))
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
            Ok(format!("Waited {timeout_ms}ms"))
        }
    }

    /// Evaluate a JavaScript expression and return the result.
    pub async fn evaluate(&self, expression: &str) -> Result<String, BrowserError> {
        cdp::evaluate_js(&self.page, expression).await
    }

    /// Drag one element to another.
    pub async fn drag(&self, ref_id: &str, target_ref_id: &str) -> Result<String, BrowserError> {
        let from_id = self
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.to_string(),
            })?;
        let to_id = self
            .refs
            .resolve(target_ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: target_ref_id.to_string(),
            })?;
        cdp::drag_node(&self.page, from_id, to_id).await?;
        Ok(format!("Dragged [ref={ref_id}] to [ref={target_ref_id}]"))
    }

    /// Resize the browser viewport.
    pub async fn resize(&mut self, width: u32, height: u32) -> Result<String, BrowserError> {
        cdp::resize_viewport(&self.page, width, height).await?;
        self.viewport_width = width;
        self.viewport_height = height;
        Ok(format!("Viewport resized to {width}x{height}"))
    }

    /// Current viewport dimensions `(width, height)`.
    pub fn viewport_size(&self) -> (u32, u32) {
        (self.viewport_width, self.viewport_height)
    }

    /// Get the current page URL.
    pub async fn current_url(&self) -> Result<String, BrowserError> {
        let result = self
            .page
            .evaluate("window.location.href")
            .await
            .map_err(|e| BrowserError::CdpError {
                message: e.to_string(),
            })?;
        Ok(result.into_value::<String>().unwrap_or_default())
    }

    /// Get the current page title.
    pub async fn current_title(&self) -> Result<String, BrowserError> {
        let result =
            self.page
                .evaluate("document.title")
                .await
                .map_err(|e| BrowserError::CdpError {
                    message: e.to_string(),
                })?;
        Ok(result.into_value::<String>().unwrap_or_default())
    }
}
