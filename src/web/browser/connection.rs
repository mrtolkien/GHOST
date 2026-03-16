use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use tracing::debug;

use super::cdp;
use super::error::BrowserError;
use super::tab::TabState;

const MAX_TABS: usize = 5;

const RECONNECT_DELAYS: &[Duration] = &[
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(30),
];

/// Internal connection state for a browser instance.
enum ConnectionState {
    Disconnected,
    Connected(Box<ConnectedState>),
    Failed {
        _last_error: String,
        retry_after: Instant,
    },
}

struct ConnectedState {
    browser: chromiumoxide::Browser,
    handler: JoinHandle<()>,
}

/// A single browser instance with its tabs and connection state.
///
/// Manages the CDP connection lifecycle, reconnection with backoff,
/// and per-browser tab collection.
pub struct ManagedBrowser {
    pub name: String,
    pub cdp_url: String,
    pub discovered: bool,
    connection: ConnectionState,
    pub tabs: HashMap<u32, TabState>,
    pub active_tab_id: Option<u32>,
    reconnect_attempts: usize,
    pub viewport_width: u32,
    pub viewport_height: u32,
}

impl Drop for ManagedBrowser {
    fn drop(&mut self) {
        if let ConnectionState::Connected(state) = &self.connection {
            state.handler.abort();
        }
    }
}

impl ManagedBrowser {
    pub fn new(name: String, cdp_url: String, discovered: bool) -> Self {
        Self {
            name,
            cdp_url,
            discovered,
            connection: ConnectionState::Disconnected,
            tabs: HashMap::new(),
            active_tab_id: None,
            reconnect_attempts: 0,
            viewport_width: cdp::VIEWPORT_WIDTH,
            viewport_height: cdp::VIEWPORT_HEIGHT,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.connection, ConnectionState::Connected(_))
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Ensure the browser is connected, reconnecting if needed.
    ///
    /// If the CDP event handler has finished (connection dropped),
    /// attempts reconnection with exponential backoff.
    pub async fn ensure_connected(&mut self) -> Result<(), BrowserError> {
        let needs_connect = match &self.connection {
            ConnectionState::Connected(state) => {
                if state.handler.is_finished() {
                    debug!(
                        name = %self.name,
                        "CDP handler finished, reconnecting"
                    );
                    true
                } else {
                    false
                }
            }
            ConnectionState::Failed { retry_after, .. } => {
                if Instant::now() < *retry_after {
                    return Err(BrowserError::ReconnectExhausted {
                        name: self.name.clone(),
                        attempts: self.reconnect_attempts,
                        reason: "retry backoff in progress".into(),
                    });
                }
                true
            }
            ConnectionState::Disconnected => true,
        };

        if needs_connect {
            self.attempt_connect().await?;
        }
        Ok(())
    }

    /// Try to connect (or reconnect) to the browser's CDP endpoint.
    async fn attempt_connect(&mut self) -> Result<(), BrowserError> {
        match cdp::connect(&self.cdp_url).await {
            Ok((browser, handler)) => {
                self.reconnect_attempts = 0;
                self.connection =
                    ConnectionState::Connected(Box::new(ConnectedState { browser, handler }));
                debug!(name = %self.name, "connected to browser");
                Ok(())
            }
            Err(e) => {
                self.reconnect_attempts += 1;
                let delay_idx = (self.reconnect_attempts - 1).min(RECONNECT_DELAYS.len() - 1);
                let delay = RECONNECT_DELAYS[delay_idx];
                self.connection = ConnectionState::Failed {
                    _last_error: e.to_string(),
                    retry_after: Instant::now() + delay,
                };

                if self.reconnect_attempts >= RECONNECT_DELAYS.len() {
                    Err(BrowserError::ReconnectExhausted {
                        name: self.name.clone(),
                        attempts: self.reconnect_attempts,
                        reason: e.to_string(),
                    })
                } else {
                    Err(BrowserError::ConnectionLost {
                        name: self.name.clone(),
                        reason: e.to_string(),
                    })
                }
            }
        }
    }

    /// Check if the CDP connection is alive.
    pub fn check_health(&self) -> bool {
        matches!(
            &self.connection,
            ConnectionState::Connected(state)
            if !state.handler.is_finished()
        )
    }

    /// Open a new tab, optionally navigating to a URL.
    pub async fn open_tab(
        &mut self,
        url: Option<&str>,
        tab_id: u32,
    ) -> Result<&mut TabState, BrowserError> {
        if self.tabs.len() >= MAX_TABS {
            return Err(BrowserError::TabLimitReached { limit: MAX_TABS });
        }

        let browser = match &self.connection {
            ConnectionState::Connected(state) => &state.browser,
            _ => {
                return Err(BrowserError::NoBrowserActive);
            }
        };

        let page = cdp::new_page(browser).await?;
        let target_id = page.target_id().inner().clone();

        if let Some(u) = url {
            cdp::navigate(&page, u).await?;
        }

        let tab = TabState::new(tab_id, page, target_id);
        self.tabs.insert(tab_id, tab);
        self.active_tab_id = Some(tab_id);

        Ok(self.tabs.get_mut(&tab_id).expect("just inserted"))
    }

    /// Close a tab by ID.
    pub fn close_tab(&mut self, tab_id: u32) -> Result<(), BrowserError> {
        if self.tabs.remove(&tab_id).is_none() {
            return Err(BrowserError::TabNotFound { id: tab_id });
        }
        // If we closed the active tab, pick another.
        if self.active_tab_id == Some(tab_id) {
            self.active_tab_id = self.tabs.keys().next().copied();
        }
        Ok(())
    }

    /// Get a reference to the active tab.
    pub fn active_tab(&self) -> Option<&TabState> {
        self.active_tab_id.and_then(|id| self.tabs.get(&id))
    }

    /// Get a mutable reference to the active tab.
    pub fn active_tab_mut(&mut self) -> Option<&mut TabState> {
        self.active_tab_id.and_then(|id| self.tabs.get_mut(&id))
    }

    /// Disconnect from the browser, aborting the handler and
    /// clearing tabs.
    pub fn disconnect(&mut self) {
        if let ConnectionState::Connected(state) = &self.connection {
            state.handler.abort();
        }
        self.connection = ConnectionState::Disconnected;
        self.tabs.clear();
        self.active_tab_id = None;
    }
}

impl std::fmt::Debug for ManagedBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedBrowser")
            .field("name", &self.name)
            .field("cdp_url", &self.cdp_url)
            .field("connected", &self.is_connected())
            .field("tab_count", &self.tabs.len())
            .field("active_tab_id", &self.active_tab_id)
            .finish()
    }
}
