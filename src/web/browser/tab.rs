use super::accessibility::RefMap;
use super::error::BrowserError;

/// State for a single browser tab.
///
/// Holds the CDP page handle and a per-tab ref map that maps
/// snapshot-assigned ref IDs to backend node IDs.
pub struct TabState {
    pub id: u32,
    pub page: chromiumoxide::Page,
    pub refs: RefMap,
    /// CDP target ID (internal, not exposed to the LLM).
    pub target_id: String,
}

/// Tab metadata for listings (no page handle).
#[derive(Debug, Clone)]
pub struct TabInfo {
    pub id: u32,
    pub url: String,
    pub title: String,
}

impl TabState {
    pub fn new(id: u32, page: chromiumoxide::Page, target_id: String) -> Self {
        Self {
            id,
            page,
            refs: RefMap::new(),
            target_id,
        }
    }

    /// Current page URL via JS eval.
    pub async fn url(&self) -> String {
        self.page
            .evaluate("window.location.href")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default()
    }

    /// Current page title via JS eval.
    pub async fn title(&self) -> String {
        self.page
            .evaluate("document.title")
            .await
            .ok()
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default()
    }

    /// Build a [`TabInfo`] snapshot of this tab's metadata.
    pub async fn info(&self) -> TabInfo {
        TabInfo {
            id: self.id,
            url: self.url().await,
            title: self.title().await,
        }
    }

    /// Get the current page URL, returning a [`BrowserError`] on failure.
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

    /// Get the current page title, returning a [`BrowserError`] on failure.
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

impl std::fmt::Debug for TabState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabState")
            .field("id", &self.id)
            .field("target_id", &self.target_id)
            .finish_non_exhaustive()
    }
}
