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
        })
    }

    pub async fn navigate(&mut self, url: &str) -> Result<(String, String), BrowserError> {
        self.refs.reset();
        cdp::navigate(&self.page, url).await
    }

    pub async fn snapshot(&mut self, offset: usize) -> Result<String, BrowserError> {
        self.refs.reset();
        let raw_nodes = cdp::get_accessibility_tree(&self.page).await?;
        let tree = parse_ax_tree(&raw_nodes);
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
