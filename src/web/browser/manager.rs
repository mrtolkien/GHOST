use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::debug;

use super::accessibility::{parse_ax_tree, render_xml};
use super::cdp::{self, ScrollDirection};
use super::connection::ManagedBrowser;
use super::error::BrowserError;
use super::tab::{TabInfo, TabState};
use super::url_check;
use super::{MAX_SNAPSHOT_DEPTH, MAX_SNAPSHOT_NODES};
use crate::config::BrowserConfig;

/// Summary info about a browser for listings.
#[derive(Debug, Clone)]
pub struct BrowserInfo {
    pub name: String,
    pub cdp_url: String,
    pub connected: bool,
    pub tab_count: usize,
    pub discovered: bool,
}

/// Manages multiple browser instances, each with multiple tabs.
///
/// Wraps `ManagedBrowser` instances and delegates page interactions
/// to the active browser's active tab. Replaces the old
/// `BrowserSession` with multi-browser, multi-tab support.
pub struct BrowserManager {
    browsers: HashMap<String, ManagedBrowser>,
    active_browser: Option<String>,
    tab_counter: u32,
}

impl BrowserManager {
    /// Create a new manager from config-defined browser entries.
    #[must_use]
    pub fn new(browser_configs: Vec<BrowserConfig>) -> Self {
        let mut browsers = HashMap::new();
        for cfg in browser_configs {
            browsers.insert(
                cfg.name.clone(),
                ManagedBrowser::new(cfg.name, cfg.cdp_url, cfg.discovered),
            );
        }
        Self {
            browsers,
            active_browser: None,
            tab_counter: 0,
        }
    }

    fn next_tab_id(&mut self) -> u32 {
        self.tab_counter += 1;
        self.tab_counter
    }

    /// The CDP URL of the active browser, if any.
    pub fn active_cdp_url(&self) -> Option<String> {
        self.active_browser
            .as_ref()
            .and_then(|name| self.browsers.get(name))
            .map(|b| b.cdp_url.clone())
    }

    /// Summary of all registered browsers.
    pub fn list_browsers(&self) -> Vec<BrowserInfo> {
        self.browsers
            .values()
            .map(|b| BrowserInfo {
                name: b.name.clone(),
                cdp_url: b.cdp_url.clone(),
                connected: b.check_health(),
                tab_count: b.tab_count(),
                discovered: b.discovered,
            })
            .collect()
    }

    /// Get a mutable reference to the active tab, ensuring the
    /// browser is connected.
    async fn active_tab_mut(&mut self) -> Result<&mut TabState, BrowserError> {
        let browser_name = self
            .active_browser
            .clone()
            .ok_or(BrowserError::NoBrowserActive)?;
        let browser = self
            .browsers
            .get_mut(&browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        browser.ensure_connected().await?;
        browser.active_tab_mut().ok_or(BrowserError::NoTabActive)
    }

    /// Auto-activate the first browser if none is active.
    fn auto_activate_browser(&mut self) -> Result<(), BrowserError> {
        if self.active_browser.is_none() {
            if let Some(name) = self.browsers.keys().next().cloned() {
                self.active_browser = Some(name);
            } else {
                return Err(BrowserError::NoBrowserActive);
            }
        }
        Ok(())
    }

    /// Navigate to a URL on the active tab.
    ///
    /// Auto-activates the first browser and opens a tab if needed
    /// (preserving the old lazy-init behaviour).
    pub async fn navigate(&mut self, url: &str) -> Result<(String, String), BrowserError> {
        self.auto_activate_browser()?;
        let browser_name = self
            .active_browser
            .clone()
            .ok_or(BrowserError::NoBrowserActive)?;
        let browser = self
            .browsers
            .get_mut(&browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        browser.ensure_connected().await?;

        // Auto-open a tab if there are none.
        if browser.active_tab_id.is_none() {
            let tab_id = self.next_tab_id();
            let browser = self
                .browsers
                .get_mut(&browser_name)
                .ok_or(BrowserError::NoBrowserActive)?;
            browser.open_tab(None, tab_id).await?;
        }

        let tab = self.active_tab_mut().await?;
        url_check::validate_url(url)?;
        tab.refs.reset();
        cdp::navigate(&tab.page, url).await
    }

    /// Get an accessibility tree snapshot of the active tab.
    pub async fn snapshot(&mut self, offset: usize) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        tab.refs.reset();
        let raw_nodes = cdp::get_accessibility_tree(&tab.page).await?;
        debug!(node_count = raw_nodes.len(), "raw AX tree fetched");
        let tree = parse_ax_tree(&raw_nodes);
        debug!(tree_size = tree.len(), "parsed AX tree");
        let xml = render_xml(
            &tree,
            &mut tab.refs,
            MAX_SNAPSHOT_NODES,
            MAX_SNAPSHOT_DEPTH,
            offset,
        );
        Ok(xml)
    }

    /// Click an element by ref ID on the active tab.
    pub async fn click(&mut self, ref_id: &str) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let node_id = tab
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.into(),
            })?;
        cdp::click_node(&tab.page, node_id).await?;
        Ok(format!("Clicked [ref={ref_id}]"))
    }

    /// Type text into an element by ref ID on the active tab.
    pub async fn type_text(&mut self, ref_id: &str, text: &str) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let node_id = tab
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.into(),
            })?;
        cdp::type_into_node(&tab.page, node_id, text).await?;
        Ok(format!("Typed into [ref={ref_id}]"))
    }

    /// Scroll the page or an element on the active tab.
    pub async fn scroll(
        &mut self,
        direction: ScrollDirection,
        ref_id: Option<&str>,
    ) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let node_id = ref_id
            .map(|r| {
                tab.refs
                    .resolve(r)
                    .ok_or_else(|| BrowserError::RefNotFound {
                        ref_id: r.to_string(),
                    })
            })
            .transpose()?;
        cdp::scroll(&tab.page, direction, node_id).await?;
        let dir = match direction {
            ScrollDirection::Up => "up",
            ScrollDirection::Down => "down",
        };
        Ok(format!("Scrolled {dir}"))
    }

    /// Capture a screenshot of the active tab.
    pub async fn screenshot(&mut self, workspace: &Path) -> Result<PathBuf, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let bytes = cdp::screenshot(&tab.page).await?;
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

    /// Send a keyboard key press on the active tab.
    pub async fn press(&mut self, key: &str) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        cdp::press_key(&tab.page, key).await?;
        Ok(format!("Pressed {key}"))
    }

    /// Hover over an element by ref ID on the active tab.
    pub async fn hover(&mut self, ref_id: &str) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let node_id = tab
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.into(),
            })?;
        cdp::hover_node(&tab.page, node_id).await?;
        Ok(format!("Hovered [ref={ref_id}]"))
    }

    /// Select an option in a `<select>` by ref ID on the active tab.
    pub async fn select(&mut self, ref_id: &str, value: &str) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let node_id = tab
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.into(),
            })?;
        cdp::select_option(&tab.page, node_id, value).await?;
        Ok(format!("Selected '{value}' in [ref={ref_id}]"))
    }

    /// Fill multiple form fields, clearing each before typing.
    pub async fn fill(&mut self, fields: &[(String, String)]) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        for (ref_id, value) in fields {
            let node_id = tab
                .refs
                .resolve(ref_id)
                .ok_or_else(|| BrowserError::RefNotFound {
                    ref_id: ref_id.clone(),
                })?;
            cdp::fill_field(&tab.page, node_id, value).await?;
        }
        Ok(format!("Filled {} fields", fields.len()))
    }

    /// Wait for an element or a fixed duration on the active tab.
    pub async fn wait(
        &mut self,
        ref_id: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String, BrowserError> {
        if let Some(r) = ref_id {
            let tab = self.active_tab_mut().await?;
            let node_id = tab
                .refs
                .resolve(r)
                .ok_or_else(|| BrowserError::RefNotFound {
                    ref_id: r.to_string(),
                })?;
            cdp::wait_for_element(&tab.page, node_id, timeout_ms).await?;
            Ok(format!("Waited for [ref={r}]"))
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
            Ok(format!("Waited {timeout_ms}ms"))
        }
    }

    /// Evaluate a JavaScript expression on the active tab.
    pub async fn evaluate(&mut self, expression: &str) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        cdp::evaluate_js(&tab.page, expression).await
    }

    /// Drag one element to another on the active tab.
    pub async fn drag(
        &mut self,
        ref_id: &str,
        target_ref_id: &str,
    ) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let from_id = tab
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.into(),
            })?;
        let to_id = tab
            .refs
            .resolve(target_ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: target_ref_id.into(),
            })?;
        cdp::drag_node(&tab.page, from_id, to_id).await?;
        Ok(format!("Dragged [ref={ref_id}] to [ref={target_ref_id}]"))
    }

    /// Upload files to a `<input type="file">` on the active tab.
    pub async fn upload(
        &mut self,
        ref_id: &str,
        chrome_paths: &[String],
    ) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        let node_id = tab
            .refs
            .resolve(ref_id)
            .ok_or_else(|| BrowserError::RefNotFound {
                ref_id: ref_id.into(),
            })?;
        cdp::set_file_input_files(&tab.page, node_id, chrome_paths).await?;
        let count = chrome_paths.len();
        let noun = if count == 1 { "file" } else { "files" };
        Ok(format!("Uploaded {count} {noun} to [ref={ref_id}]"))
    }

    /// Resize the browser viewport on the active tab.
    pub async fn resize(&mut self, width: u32, height: u32) -> Result<String, BrowserError> {
        // Update viewport on the managed browser.
        let browser_name = self
            .active_browser
            .clone()
            .ok_or(BrowserError::NoBrowserActive)?;

        let tab = self.active_tab_mut().await?;
        cdp::resize_viewport(&tab.page, width, height).await?;

        // Store the viewport size on the browser.
        if let Some(browser) = self.browsers.get_mut(&browser_name) {
            browser.viewport_width = width;
            browser.viewport_height = height;
        }
        Ok(format!("Viewport resized to {width}x{height}"))
    }

    /// Current viewport dimensions `(width, height)`.
    pub fn viewport_size(&self) -> (u32, u32) {
        self.active_browser
            .as_ref()
            .and_then(|name| self.browsers.get(name))
            .map(|b| (b.viewport_width, b.viewport_height))
            .unwrap_or((cdp::VIEWPORT_WIDTH, cdp::VIEWPORT_HEIGHT))
    }

    /// ID of the active tab in the active browser, if any.
    pub fn active_tab_id(&self) -> Option<u32> {
        self.active_browser
            .as_ref()
            .and_then(|name| self.browsers.get(name))
            .and_then(|b| b.active_tab_id)
    }

    /// List tabs in the active browser, sorted by tab ID.
    pub async fn list_tabs(&self) -> Result<Vec<TabInfo>, BrowserError> {
        let browser_name = self
            .active_browser
            .as_ref()
            .ok_or(BrowserError::NoBrowserActive)?;
        let browser = self
            .browsers
            .get(browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        let mut tabs = Vec::new();
        for tab in browser.tabs.values() {
            tabs.push(tab.info().await);
        }
        tabs.sort_by_key(|t| t.id);
        Ok(tabs)
    }

    /// Open a new tab in the active browser, optionally navigating to a URL.
    ///
    /// Auto-activates the first browser if none is active. Returns a
    /// snapshot of the new tab.
    pub async fn open_tab(&mut self, url: Option<&str>) -> Result<String, BrowserError> {
        self.auto_activate_browser()?;
        let browser_name = self
            .active_browser
            .clone()
            .ok_or(BrowserError::NoBrowserActive)?;
        let browser = self
            .browsers
            .get_mut(&browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        browser.ensure_connected().await?;
        let tab_id = self.next_tab_id();
        let browser = self
            .browsers
            .get_mut(&browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        browser.open_tab(url, tab_id).await?;
        self.snapshot(0).await
    }

    /// Switch the active tab to `tab_id` and return a snapshot.
    ///
    /// Resets refs on the newly focused tab so stale ref IDs from
    /// the previous tab are not accidentally reused.
    pub async fn focus_tab(&mut self, tab_id: u32) -> Result<String, BrowserError> {
        let browser_name = self
            .active_browser
            .as_ref()
            .ok_or(BrowserError::NoBrowserActive)?
            .clone();
        let browser = self
            .browsers
            .get_mut(&browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        if !browser.tabs.contains_key(&tab_id) {
            return Err(BrowserError::TabNotFound { id: tab_id });
        }
        browser.active_tab_id = Some(tab_id);
        if let Some(tab) = browser.active_tab_mut() {
            tab.refs.reset();
        }
        self.snapshot(0).await
    }

    /// Close a tab by ID. Returns a confirmation message.
    pub fn close_tab(&mut self, tab_id: u32) -> Result<String, BrowserError> {
        let browser_name = self
            .active_browser
            .as_ref()
            .ok_or(BrowserError::NoBrowserActive)?
            .clone();
        let browser = self
            .browsers
            .get_mut(&browser_name)
            .ok_or(BrowserError::NoBrowserActive)?;
        browser.close_tab(tab_id)?;
        Ok(format!("Closed tab {tab_id}"))
    }

    /// Get the current page URL on the active tab.
    pub async fn current_url(&mut self) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        tab.current_url().await
    }

    /// Get the current page title on the active tab.
    pub async fn current_title(&mut self) -> Result<String, BrowserError> {
        let tab = self.active_tab_mut().await?;
        tab.current_title().await
    }

    /// Name of the active browser, if any.
    pub fn active_browser_name(&self) -> Option<&str> {
        self.active_browser.as_deref()
    }

    /// Connect to a browser by name and CDP URL, setting it as active.
    ///
    /// If a browser with this name already exists, its URL is updated
    /// and the connection is refreshed. New browsers are added in-memory
    /// only — they are not persisted to config (`ghost browsers add`
    /// handles persistence).
    pub async fn connect_browser(
        &mut self,
        name: &str,
        cdp_url: &str,
    ) -> Result<BrowserInfo, BrowserError> {
        if let Some(browser) = self.browsers.get_mut(name) {
            if browser.cdp_url != cdp_url {
                browser.cdp_url = cdp_url.to_string();
            }
            browser.disconnect();
            browser.ensure_connected().await?;
        } else {
            let mut browser = ManagedBrowser::new(name.to_string(), cdp_url.to_string(), false);
            browser.ensure_connected().await?;
            self.browsers.insert(name.to_string(), browser);
        }
        self.active_browser = Some(name.to_string());

        let browser = self
            .browsers
            .get(name)
            .ok_or_else(|| BrowserError::BrowserNotFound { name: name.into() })?;
        Ok(BrowserInfo {
            name: browser.name.clone(),
            cdp_url: browser.cdp_url.clone(),
            connected: browser.check_health(),
            tab_count: browser.tab_count(),
            discovered: browser.discovered,
        })
    }

    /// Disconnect a browser by name.
    ///
    /// Clears the active browser selection if it was the one disconnected.
    pub fn disconnect_browser(&mut self, name: &str) -> Result<(), BrowserError> {
        let browser = self
            .browsers
            .get_mut(name)
            .ok_or_else(|| BrowserError::BrowserNotFound { name: name.into() })?;
        browser.disconnect();
        if self.active_browser.as_deref() == Some(name) {
            self.active_browser = None;
        }
        Ok(())
    }
}

impl std::fmt::Debug for BrowserManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserManager")
            .field("browser_count", &self.browsers.len())
            .field("active_browser", &self.active_browser)
            .field("tab_counter", &self.tab_counter)
            .finish()
    }
}
