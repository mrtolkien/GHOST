# Multi-Browser + Multi-Tab Browser Tool

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the browser tool to support multiple named browsers (headless sidecar,
operator's Chrome, remote instances) each with multiple tabs, with CDP discovery, config
and a browser-use skill to guide the GHOST.

**Architecture:** `BrowserManager` replaces the single `BrowserSession`, holding a map of
named `ManagedBrowser` instances each connected via CDP. Each browser manages up to 5
tabs with per-tab element refs. Config is the single source of truth for browser
definitions (`[[web.browsers]]` in config.toml). `ghost reboot` applies config changes.
CDP Target events auto-detect new tabs from link clicks. A bundled skill replaces the
verbose tool description.

**Tech Stack:** chromiumoxide (CDP), tokio, serde/toml (config write-back),
toml_edit, tracing/logfire

**Specs:**

- `backlog/tasks/1-user-facing/browser-multi-tab.md`
- `backlog/tasks/1-user-facing/browser-operator-relay.md`

**Prior art:** OpenClaw's browser tool — sticky `lastTargetId`, per-tab ref cache (up to
50 entries), `MANAGED_BROWSER_PAGE_TAB_LIMIT = 8`, session tab registry,
`Target.targetCreated` event subscription, prefix-matching on target IDs. ZeroClaw has
no multi-tab support.

---

## Design Decisions Reference

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | **Config.toml is the single source of truth** for browser definitions. No DB table. | CLI, tool actions, and discovery all write to config.toml then trigger reload. Runtime state (connections, tabs, refs) lives in memory only. |
| 2 | **Tab IDs are sequential u32** (1, 2, 3...) | LLM-friendly. CDP target IDs are internal-only. |
| 3 | **Refs invalidate on tab switch**, auto-snapshot on `focus` | Simplest model. Matches Ghost's existing "reset on every snapshot." No cross-tab ref confusion. |
| 4 | **5 tabs per browser**, error on limit (no auto-close) | Explicit is better for an agent. GHOST decides which tab to close. |
| 5 | **Reactive health detection** — errors from CDP commands trigger reconnect | No polling. Handler `JoinHandle` completing signals dropped WebSocket. Exponential backoff: 1s, 2s, 4s, 8s, 30s max. |
| 6 | **Sticky active_browser + active_tab** — updated by connect/open/focus | All interaction actions implicitly operate on active tab of active browser. Zero-overhead for single-browser single-tab (today's common case). |
| 7 | **Browser tool always registered** (runtime check, not config-time) | Lets GHOST use `discover`/`connect` even when no browsers are pre-configured. Returns helpful error if no browser is available. |
| 8 | **Crawl4AI gets active browser's CDP URL or nothing** | If nothing, Crawl4AI manages its own headless Chrome. No fallback chain. |
| 9 | **No state transfer between browsers** | Cookie/localStorage export is fragile (~60% of sites). Skill guides GHOST to ask operator to start a dedicated Chromium with CDP on the Tailscale mesh instead. |
| 10 | **`ghost reboot` applies config changes** | No partial reload. CLI commands that modify config.toml (e.g. `ghost browsers add`) tell the user to `ghost reboot`. Full restart picks up all changes. Connections are lazy — they re-establish on first use. |
| 11 | **New tab auto-detection** via CDP `Target.targetCreated` events | When target=_blank opens a new tab, auto-track it and switch active_tab to it. |
| 12 | **Env var compat** — `CHROME_CDP_URL` creates a "headless" browser entry | Backwards-compatible for existing setups. Only used when no `[[web.browsers]]` defined. |

---

## File Map

### New files

| File | Responsibility |
|------|----------------|
| `src/web/browser/manager.rs` | `BrowserManager` — multi-browser orchestrator, active browser/tab tracking, high-level API |
| `src/web/browser/connection.rs` | `ManagedBrowser` — single browser connection lifecycle, reconnect with backoff, tab storage |
| `src/web/browser/tab.rs` | `TabState` — per-tab page handle + RefMap. `TabInfo` for listings. |
| `src/web/browser/discovery.rs` | CDP endpoint discovery — localhost port scan + Tailscale peer scan |
| `src/cli/browsers.rs` | CLI subcommands: `ghost browsers list|add|remove|discover|check` |
| `assets/skills/browser-use/skill.md` | Bundled skill — browser workflow guidance, replaces verbose tool description |

### Modified files

| File | Change |
|------|--------|
| `src/config.rs` | Add `browsers: Option<Vec<BrowserSettings>>` to WebSettings (keep `chrome_cdp_url` as deprecated compat); `browsers: Vec<BrowserConfig>` in WebConfig. Add `BrowserSettings`/`BrowserConfig` types. Fallback chain: browsers > chrome_cdp_url > env var. Update `test_config()` helper: `chrome_cdp_url: None, browsers: None`. |
| `src/config_cli.rs` | Add `add_browser(name, cdp_url)` and `remove_browser(name)` — array manipulation in config.toml. |
| `src/web/browser/mod.rs` | Remove `BrowserSession` struct. Re-export `BrowserManager`, `ManagedBrowser`, `TabState`, etc. Keep constants. |
| `src/web/browser/cdp.rs` | Add `subscribe_target_events()` for `Target.targetCreated`/`targetDestroyed`. Expose target ID from `new_page()`. |
| `src/web/browser/error.rs` | New variants: `ConnectionLost`, `TabLimitReached`, `NoBrowserActive`, `NoTabActive`, `BrowserNotFound`, `TabNotFound`, `DiscoveryFailed`. |
| `src/tools/browser.rs` | New actions (browsers, connect, disconnect, discover, tabs, open, focus, close). Use `BrowserManager` instead of `BrowserSession`. Slim tool description → point to skill. |
| `src/tools/context.rs` | Replace `browser_session: Arc<TokioMutex<Option<BrowserSession>>>` with `browser_manager: Arc<TokioMutex<BrowserManager>>`. |
| `src/tools/manager.rs` | Always register browser tool (remove `with_browser_if_configured` conditional). |
| `src/chat/session.rs` | Store `browser_manager: Arc<TokioMutex<BrowserManager>>` instead of `browser_session`. Pass to `ToolContext`. |
| `src/daemon/run.rs` | Create `BrowserManager` at boot, pass to `SessionChat`. |
| `src/web/fetch.rs` | Get CDP URL from `BrowserManager.active_cdp_url()` instead of `config.web.chrome_cdp_url`. Pass `None` if no active browser (let Crawl4AI handle it). |
| `src/cli/mod.rs` | Register `browsers` subcommand. |
| `src/main.rs` | Wire `Browsers` CLI variant. |
| `src/scripting/bindings.rs` | Update `ToolContext` construction: `browser_session` → `browser_manager`. |
| `tests/common.rs` | Update test helper `ToolContext` construction. |
| `tests/browser_live.rs` | Update to use `BrowserManager` instead of `BrowserSession`. |
| Other test files constructing `ToolContext` | Grep for `browser_session`, update all occurrences. |

---

## Chunk 1: Foundation — Config + BrowserManager + Wiring

### Task 1: Config schema change

**Files:**

- Modify: `src/config.rs`

- [ ] **Step 1: Add BrowserSettings and BrowserConfig types**

```rust
/// A browser definition in config.toml.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BrowserSettings {
    pub name: String,
    pub cdp_url: String,
    /// Marker for browsers added by `discover` (not manually).
    #[serde(default)]
    pub discovered: bool,
}

/// Resolved browser configuration.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserConfig {
    pub name: String,
    pub cdp_url: String,
    pub discovered: bool,
}
```

- [ ] **Step 2: Update WebSettings**

Add `browsers` field alongside `chrome_cdp_url` (kept for backwards compatibility since
`WebSettings` uses `deny_unknown_fields` — existing configs with `chrome_cdp_url` would
fail to parse if we removed it). The deprecated field is consumed during resolution and
never used at runtime.

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub search_max_results: Option<usize>,
    pub crawl4ai_url: Option<String>,
    /// Deprecated: use [[web.browsers]] instead. Kept for config compat.
    pub chrome_cdp_url: Option<String>,
    pub browsers: Option<Vec<BrowserSettings>>,
    pub search: Option<SearchProviderSettings>,
}
```

- [ ] **Step 3: Update WebConfig**

```rust
#[derive(Debug, Clone, Serialize)]
pub struct WebConfig {
    pub search_max_results: usize,
    pub crawl4ai_url: Option<String>,
    pub browsers: Vec<BrowserConfig>,
    pub search_provider: SearchProviderConfig,
}
```

- [ ] **Step 4: Update config resolution**

In the `Config::from_settings` method, replace the `chrome_cdp_url` resolution block
with browser list resolution. Fallback chain: `[[web.browsers]]` > `chrome_cdp_url`
(deprecated) > `CHROME_CDP_URL` env var:

```rust
let browsers = {
    let configured = settings
        .web
        .as_ref()
        .and_then(|w| w.browsers.clone())
        .unwrap_or_default();

    if !configured.is_empty() {
        configured
            .into_iter()
            .map(|b| BrowserConfig {
                name: b.name,
                cdp_url: b.cdp_url,
                discovered: b.discovered,
            })
            .collect()
    } else {
        // Deprecated config field fallback
        let legacy_url = settings
            .web
            .as_ref()
            .and_then(|w| w.chrome_cdp_url.clone())
            .or_else(|| env::var("CHROME_CDP_URL").ok());

        if let Some(url) = legacy_url {
            vec![BrowserConfig {
                name: "headless".to_string(),
                cdp_url: url,
                discovered: false,
            }]
        } else {
            vec![]
        }
    }
};
```

- [ ] **Step 5: Fix all compile errors from chrome_cdp_url removal**

Grep for `chrome_cdp_url` across the codebase. Update:
- `src/tools/browser.rs` — will be migrated in Task 4
- `src/tools/manager.rs` — will be migrated in Task 4
- `src/web/fetch.rs` — will be migrated in Task 4

For now, temporarily use `config.web.browsers.first().map(|b| b.cdp_url.as_str())` in
place of `config.web.chrome_cdp_url.as_deref()` at each call site. Mark each with
`// TODO(multi-browser): use BrowserManager.active_cdp_url()` for Task 4.

- [ ] **Step 6: Add config parsing test**

Add a unit test that parses a TOML string with `[[web.browsers]]` and verifies the
resolved `WebConfig.browsers` vec. Also test the `CHROME_CDP_URL` env var fallback
(temporarily set env var, parse empty config, verify single "headless" entry).

- [ ] **Step 7: Run `just ci`, fix any issues**

- [ ] **Step 8: Commit**

```
feat: replace chrome_cdp_url with [[web.browsers]] config array
```

---

### Task 2: Error types

**Files:**

- Modify: `src/web/browser/error.rs`

- [ ] **Step 1: Add new error variants**

```rust
#[derive(Debug, Error)]
pub enum BrowserError {
    // ... existing variants ...

    #[error("no browser is active — connect to a browser first")]
    NoBrowserActive,

    #[error("no tab is active — open a tab first")]
    NoTabActive,

    #[error("browser '{name}' not found")]
    BrowserNotFound { name: String },

    #[error("tab {id} not found")]
    TabNotFound { id: u32 },

    #[error("tab limit reached ({limit} tabs) — close a tab first")]
    TabLimitReached { limit: usize },

    #[error("browser '{name}' connection lost: {reason}. reconnect in progress")]
    ConnectionLost { name: String, reason: String },

    #[error("browser '{name}' reconnect exhausted after {attempts} attempts: {reason}")]
    ReconnectExhausted {
        name: String,
        attempts: usize,
        reason: String,
    },

    #[error("CDP discovery failed: {reason}")]
    DiscoveryFailed { reason: String },
}
```

- [ ] **Step 2: Run `just ci`, fix any issues**

- [ ] **Step 3: Commit**

```
feat(browser): add multi-browser error variants
```

---

### Task 3: Core types — TabState, ManagedBrowser, BrowserManager

**Files:**

- Create: `src/web/browser/tab.rs`
- Create: `src/web/browser/connection.rs`
- Create: `src/web/browser/manager.rs`
- Modify: `src/web/browser/mod.rs` (add `pub mod` declarations)

- [ ] **Step 1: Create `tab.rs`**

```rust
use super::accessibility::RefMap;

/// State for a single browser tab.
pub struct TabState {
    pub id: u32,
    pub page: chromiumoxide::Page,
    pub refs: RefMap,
    /// CDP target ID (internal — not exposed to the LLM).
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

    /// Get current URL via JS eval.
    pub async fn url(&self) -> String {
        self.page
            .evaluate("window.location.href")
            .await
            .ok()
            .and_then(|r| r.into_value::<String>().ok())
            .unwrap_or_default()
    }

    /// Get current title via JS eval.
    pub async fn title(&self) -> String {
        self.page
            .evaluate("document.title")
            .await
            .ok()
            .and_then(|r| r.into_value::<String>().ok())
            .unwrap_or_default()
    }

    /// Build a TabInfo snapshot.
    pub async fn info(&self) -> TabInfo {
        TabInfo {
            id: self.id,
            url: self.url().await,
            title: self.title().await,
        }
    }
}
```

- [ ] **Step 2: Create `connection.rs`**

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;

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
const MAX_RECONNECT_ATTEMPTS: usize = 5;

/// Manual Debug impl needed — chromiumoxide::Browser doesn't derive Debug.
pub enum ConnectionState {
    Disconnected,
    Connected {
        browser: chromiumoxide::Browser,
        handler: JoinHandle<()>,
    },
    Failed {
        last_error: String,
        retry_after: Instant,
    },
}

/// A single browser connection with its tabs.
pub struct ManagedBrowser {
    pub name: String,
    pub cdp_url: String,
    pub discovered: bool,
    connection: ConnectionState,
    pub tabs: HashMap<u32, TabState>,
    pub active_tab_id: Option<u32>,
    reconnect_attempts: usize,
}

impl Drop for ManagedBrowser {
    fn drop(&mut self) {
        if let ConnectionState::Connected { handler, .. } = &self.connection {
            handler.abort();
        }
    }
}
```

Then implement key methods:

```rust
impl ManagedBrowser {
    pub fn new(name: String, cdp_url: String, discovered: bool) -> Self { /* ... */ }

    pub fn is_connected(&self) -> bool {
        matches!(self.connection, ConnectionState::Connected { .. })
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Get the chromiumoxide Browser handle, reconnecting if needed.
    ///
    /// On reconnect, all tabs are lost — cleared from state.
    /// Returns clear error if in Failed state and retry_after hasn't elapsed.
    pub async fn ensure_connected(
        &mut self,
    ) -> Result<&chromiumoxide::Browser, BrowserError> {
        // 1. If Connected, check if handler has finished (connection dropped).
        //    If handler.is_finished(), mark Disconnected, fall through.
        // 2. If Failed and retry_after not elapsed, return error.
        // 3. If Disconnected or Failed (retry_after elapsed):
        //    a. Clear all tabs (connections are stale).
        //    b. Attempt CDP connect with timeout (5s).
        //    c. On success: set Connected, reset reconnect_attempts.
        //    d. On failure: increment reconnect_attempts, compute next delay
        //       from RECONNECT_DELAYS (clamped), set Failed.
        todo!()
    }

    /// Check if the connection is alive without side effects.
    pub fn check_health(&self) -> bool {
        match &self.connection {
            ConnectionState::Connected { handler, .. } => !handler.is_finished(),
            _ => false,
        }
    }

    /// Open a new tab. Enforces MAX_TABS limit.
    pub async fn open_tab(
        &mut self,
        url: Option<&str>,
        tab_id: u32,
    ) -> Result<&mut TabState, BrowserError> {
        if self.tabs.len() >= MAX_TABS {
            return Err(BrowserError::TabLimitReached { limit: MAX_TABS });
        }
        let browser = self.ensure_connected().await?;
        // Use cdp::new_page, navigate if url provided.
        // Get target_id from page.
        // Create TabState, insert into self.tabs, set active_tab_id.
        todo!()
    }

    /// Close a tab by ID.
    pub async fn close_tab(&mut self, tab_id: u32) -> Result<(), BrowserError> {
        // Remove from self.tabs. If active_tab_id was this tab,
        // set active_tab_id to another tab or None.
        // Call page.close() via CDP.
        todo!()
    }

    pub fn active_tab(&self) -> Option<&TabState> {
        self.active_tab_id.and_then(|id| self.tabs.get(&id))
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut TabState> {
        self.active_tab_id.and_then(|id| self.tabs.get_mut(&id))
    }
}
```

- [ ] **Step 3: Create `manager.rs`**

```rust
use std::collections::HashMap;

use crate::config::BrowserConfig;

use super::accessibility::RefMap;
use super::connection::ManagedBrowser;
use super::error::BrowserError;
use super::tab::{TabInfo, TabState};
use super::{MAX_SNAPSHOT_DEPTH, MAX_SNAPSHOT_NODES};

/// Browser info for listings.
#[derive(Debug, Clone)]
pub struct BrowserInfo {
    pub name: String,
    pub cdp_url: String,
    pub connected: bool,
    pub tab_count: usize,
    pub discovered: bool,
}

pub struct BrowserManager {
    browsers: HashMap<String, ManagedBrowser>,
    active_browser: Option<String>,
    tab_counter: u32,
}
```

Then implement the high-level API that delegates to active browser/tab:

```rust
impl BrowserManager {
    pub fn new(browser_configs: Vec<BrowserConfig>) -> Self {
        let browsers = browser_configs
            .into_iter()
            .map(|c| {
                (
                    c.name.clone(),
                    ManagedBrowser::new(c.name, c.cdp_url, c.discovered),
                )
            })
            .collect();
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

    /// Get the CDP URL of the active browser (for Crawl4AI integration).
    /// Returns owned String so callers can drop the BrowserManager lock
    /// before making async calls.
    pub fn active_cdp_url(&self) -> Option<String> {
        self.active_browser
            .as_ref()
            .and_then(|name| self.browsers.get(name))
            .map(|b| b.cdp_url.clone())
    }

    // --- Browser management ---

    pub fn list_browsers(&self) -> Vec<BrowserInfo> { /* ... */ }

    /// Connect to a browser by name (must be in config) or by cdp_url
    /// (creates a new entry). Sets it as active.
    pub async fn connect_browser(
        &mut self,
        name: &str,
        cdp_url: Option<&str>,
    ) -> Result<BrowserInfo, BrowserError> {
        // If name exists in self.browsers, ensure_connected on it.
        // If not and cdp_url provided, create new ManagedBrowser, insert, connect.
        // Set active_browser = name.
        todo!()
    }

    pub async fn disconnect_browser(
        &mut self,
        name: &str,
    ) -> Result<(), BrowserError> { /* ... */ }

    // --- Tab management ---

    pub async fn list_tabs(&self) -> Result<Vec<TabInfo>, BrowserError> {
        // Return tabs from active browser. Error if no active browser.
        // Calls tab.info() (async) for each tab to get live URL/title.
        todo!()
    }

    /// Open a new tab in the active browser. If no browser is active,
    /// auto-connect to the first configured browser.
    pub async fn open_tab(
        &mut self,
        url: Option<&str>,
    ) -> Result<String, BrowserError> {
        // Auto-activate first browser if none active.
        // Call active_browser.open_tab(url, next_tab_id()).
        // Return snapshot of the new tab.
        todo!()
    }

    /// Switch active tab. Invalidates old refs. Returns snapshot of new tab.
    pub async fn focus_tab(&mut self, tab_id: u32) -> Result<String, BrowserError> {
        // Find the tab across all browsers (or just active browser?).
        // Set active_tab_id. Take snapshot. Return XML.
        todo!()
    }

    pub async fn close_tab(&mut self, tab_id: u32) -> Result<String, BrowserError> {
        todo!()
    }

    // --- Interaction (delegates to active tab) ---

    /// Get a mutable reference to the active tab, ensuring the browser is connected.
    ///
    /// Returns `&mut TabState` which gives access to both `tab.page` and `tab.refs`.
    /// This avoids the split-borrow problem: callers use one `&mut TabState` reference
    /// to access both fields, instead of separate `active_page()` / `active_refs_mut()`
    /// helpers that would conflict on `&mut self`.
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
        browser
            .active_tab_mut()
            .ok_or(BrowserError::NoTabActive)
    }

    // Then: navigate(), snapshot(), click(), type_text(), scroll(),
    // screenshot(), press(), hover(), select(), fill(), wait(),
    // evaluate(), drag(), upload(), resize()
    //
    // Each follows the same pattern as the old BrowserSession methods,
    // but calls self.active_tab_mut().await? to get `tab`, then uses
    // `tab.page` and `tab.refs` directly. Example:
    //
    //   pub async fn click(&mut self, ref_id: &str) -> Result<String, BrowserError> {
    //       let tab = self.active_tab_mut().await?;
    //       let node_id = tab.refs.resolve(ref_id)
    //           .ok_or_else(|| BrowserError::RefNotFound { ref_id: ref_id.into() })?;
    //       cdp::click_node(&tab.page, node_id).await?;
    //       Ok(format!("Clicked [ref={ref_id}]"))
    //   }
    //
    // Copy implementations from the current BrowserSession in mod.rs,
    // replacing `self.page` → `tab.page` and `self.refs` → `tab.refs`.
}
```

- [ ] **Step 4: Update `mod.rs`**

Remove the `BrowserSession` struct and its `impl` block. Add module declarations:

```rust
pub mod accessibility;
pub mod cdp;
pub mod connection;
pub mod error;
pub mod manager;
pub mod tab;
pub mod url_check;

pub use self::cdp::ScrollDirection;
pub use self::error::BrowserError;
pub use self::manager::BrowserManager;

const MAX_SNAPSHOT_NODES: usize = 500;
const MAX_SNAPSHOT_DEPTH: usize = 15;
```

- [ ] **Step 5: Wire BrowserManager into ToolContext + migrate existing actions**

This step completes the migration — all existing browser actions work through
`BrowserManager` with zero behavioral change (single browser, single tab).

**Additional files to modify** (beyond Task 3's files):
- `src/tools/context.rs`
- `src/tools/browser.rs`
- `src/tools/manager.rs`
- `src/chat/session.rs`
- `src/daemon/run.rs`
- `src/web/fetch.rs`
- All files constructing `ToolContext` in tests/scripting (grep for `browser_session`):
  `tests/browser_live.rs`, `tests/common.rs`, `src/scripting/bindings.rs`,
  and any other test files that build `ToolContext` instances.

Update ToolContext:

```rust
// In src/tools/context.rs:
// Replace:
//   pub browser_session: Arc<TokioMutex<Option<BrowserSession>>>,
// With:
pub browser_manager: Arc<tokio::sync::Mutex<BrowserManager>>,
```

- [ ] **Step 6: Create BrowserManager in daemon boot**

In `src/daemon/run.rs`, `boot_with_config`:

```rust
let browser_manager = Arc::new(tokio::sync::Mutex::new(
    BrowserManager::new(config.web.browsers.clone()),
));
```

Pass `browser_manager` to `SessionChat`. Config changes require `ghost reboot` — no
runtime reload needed.

- [ ] **Step 7: Update SessionChat**

Replace `browser_session` field with `browser_manager`. Update `execute_single_tool` to
pass `browser_manager` in `ToolContext`.

- [ ] **Step 8: Update tool manager**

In `src/tools/manager.rs`, remove the `with_browser_if_configured` conditional. Always
include the browser tool in `for_chat()`.

- [ ] **Step 9: Migrate browser tool actions**

In `src/tools/browser.rs`, replace all `ctx.browser_session.lock().await` patterns with
`ctx.browser_manager.lock().await`. The old pattern was:

```rust
// Old: lazy-init BrowserSession
let mut guard = ctx.browser_session.lock().await;
let session = match guard.as_mut() {
    Some(s) => s,
    None => {
        let cdp_url = ctx.config.web.chrome_cdp_url.as_deref()
            .ok_or_else(|| /* error */)?;
        *guard = Some(BrowserSession::connect(cdp_url).await?);
        guard.as_mut().unwrap()
    }
};
session.navigate(url).await
```

New pattern:

```rust
// New: BrowserManager handles everything
let mut mgr = ctx.browser_manager.lock().await;
mgr.navigate(url).await
```

`BrowserManager.navigate()` internally:
1. Auto-activates the first configured browser if none is active.
2. Auto-opens a tab (assigned the next sequential ID, e.g. tab 1) if the active browser
   has no tabs. This auto-created tab is tracked normally in the tab map.
3. Calls `ensure_connected()` on the active browser (handles reconnect).
4. Delegates to `cdp::navigate()` on the active tab's page.

This auto-activation ensures the single-browser experience is identical to today: the
first `navigate` call connects lazily and creates a tab.

- [ ] **Step 10: Update web_fetch Crawl4AI integration**

In `src/web/fetch.rs`, replace:

```rust
// Old:
let cdp_url = ctx.config.web.chrome_cdp_url.as_deref();

// New:
let cdp_url = ctx.browser_manager.lock().await.active_cdp_url();
// active_cdp_url() returns Option<String>, so the lock is dropped immediately.
// Pass cdp_url.as_deref() to crawl4ai. If None, Crawl4AI uses its own browser.
```

- [ ] **Step 11: Run `just ci`, fix all compile errors and warnings**

Expect updates needed in test helpers and scripting bindings that construct `ToolContext`.
Grep for `browser_session` and update every occurrence.

- [ ] **Step 12: Run existing browser live tests (if any)**

```bash
cargo test --features live-tests browser
```

Verify zero behavioral regression.

- [ ] **Step 13: Commit**

```
feat(browser): add BrowserManager, replace BrowserSession

Introduces BrowserManager, ManagedBrowser, TabState types and wires
them through ToolContext. All existing actions work identically —
single browser, single tab, lazy connection.
```

---

## Chunk 2: Multi-Tab

### Task 4: Tab actions — open, focus, close, tabs

**Files:**

- Modify: `src/tools/browser.rs` (add new action variants + handlers)
- Modify: `src/web/browser/manager.rs` (implement tab methods)
- Modify: `src/web/browser/connection.rs` (implement open_tab, close_tab)

- [ ] **Step 1: Add action variants to tool schema**

In the browser tool's action enum, add: `tabs`, `open`, `focus`, `close`.

New tool parameters:
- `tab` (u32, optional) — used by `focus` and `close`
- `browser` (string, optional) — used by `tabs` to target a specific browser

- [ ] **Step 2: Implement `tabs` action handler**

Calls `mgr.list_tabs()`. Format as:

```
Open tabs (browser: headless):
  Tab 1: Example Page — https://example.com  [active]
  Tab 2: Search Results — https://google.com/...
```

- [ ] **Step 3: Implement `open` action handler**

Calls `mgr.open_tab(url)`. The method:
1. Auto-activates first browser if none active.
2. Ensures connection.
3. Checks tab limit (5). Returns `TabLimitReached` error if at limit.
4. Creates new page via `cdp::new_page()`.
5. Navigates if URL provided.
6. Creates `TabState` with `next_tab_id()`.
7. Sets as active tab.
8. Takes snapshot. Returns snapshot XML.

- [ ] **Step 4: Implement `focus` action handler**

Calls `mgr.focus_tab(tab_id)`. The method:
1. Finds tab by ID in active browser's tabs.
2. Sets as active_tab_id.
3. Resets refs on the newly focused tab (invalidate).
4. Takes snapshot (auto-snapshot on focus).
5. Returns snapshot XML.

Note: `focus` does NOT clear the old tab's refs — they're just orphaned. The new tab
gets a fresh snapshot with fresh refs. The LLM only ever uses refs from the last
snapshot, which is always the active tab.

- [ ] **Step 5: Implement `close` action handler**

Calls `mgr.close_tab(tab_id)`. The method:
1. Finds tab in active browser.
2. Calls `page.execute(CloseTargetParams::new(target_id))` via CDP.
3. Removes TabState from the map.
4. If closed tab was active: set active to another tab (if any) or None.
5. Returns confirmation message.

- [ ] **Step 6: Update tool description**

Add brief descriptions for each new action. Full guidance will be in the skill (Task 12).

- [ ] **Step 7: Write live test for multi-tab**

Feature-gate with `live-tests`. Test flow:
1. Navigate to a URL (auto-creates first tab).
2. `open` a second tab with a different URL.
3. `tabs` — verify two tabs listed.
4. `focus` tab 1 — verify snapshot shows first URL.
5. `focus` tab 2 — verify snapshot shows second URL.
6. `close` tab 1 — verify only tab 2 remains.

- [ ] **Step 8: Run `just ci`**

- [ ] **Step 9: Commit**

```
feat(browser): add multi-tab support — open, focus, close, tabs actions
```

---

### Task 5: CDP Target events + tab limit enforcement

**Files:**

- Modify: `src/web/browser/cdp.rs`
- Modify: `src/web/browser/connection.rs`

- [ ] **Step 1: Add target event subscription to cdp.rs**

After connecting to a browser, subscribe to `Target.targetCreated` and
`Target.targetDestroyed` events. chromiumoxide provides event listener
APIs — check the crate's `EventStream` or `CdpEventListener` types.

```rust
/// Subscribe to CDP Target events on a browser.
///
/// Returns a stream of TargetEvent that the caller should poll.
pub async fn subscribe_target_events(
    browser: &chromiumoxide::Browser,
) -> Result<impl Stream<Item = TargetEvent>, BrowserError> {
    // Use browser.event_listener() or similar chromiumoxide API.
    // Filter for "page" type targets only.
    todo!()
}

pub enum TargetEvent {
    Created { target_id: String, url: String },
    Destroyed { target_id: String },
}
```

Note: Check chromiumoxide's API for the exact method. If event subscription is not
straightforward, an alternative is to poll `browser.pages()` after each action that
might create a new tab (click). Document the chosen approach.

- [ ] **Step 2: Handle new tabs in ManagedBrowser**

When a `TargetEvent::Created` is received:
1. Create a new `TabState` for the new target.
2. Set it as active tab (matches user expectation after clicking a link).
3. The next action on the active tab will use this new tab.

When `TargetEvent::Destroyed`:
1. Remove the tab from the map.
2. If it was active, select another or None.

- [ ] **Step 3: Spawn event listener task per connected browser**

In `ManagedBrowser::ensure_connected()`, after a successful connection, spawn a
`tokio::spawn` task that reads from the target event stream and updates the browser's
tab state. This requires the tab state to be behind an `Arc<Mutex<>>` or use a channel
to send events back.

Design note: Since `ManagedBrowser` is already behind `Arc<TokioMutex<BrowserManager>>`,
the event listener task can't directly mutate it (would deadlock). Instead, use a
`tokio::sync::mpsc` channel:
- Event listener sends `TargetEvent` into the channel.
- At the start of each BrowserManager method, drain pending events from the channel.
- This avoids locking issues and keeps event processing synchronous with tool calls.

**Timing gap note:** Between a `click` that opens a new tab and the next BrowserManager
method call, the CDP event may not have arrived yet. This is acceptable — the GHOST's
next `snapshot` or `tabs` call will drain the event and pick up the new tab. If immediate
detection is critical, consider polling `browser.pages()` after click actions as a
fallback.

- [ ] **Step 4: Write live test for auto-detected tabs**

Open a page with a link that has `target="_blank"`. Click the link. Verify that:
1. A new tab appears in `list_tabs()`.
2. The new tab is the active tab.

This test may be fragile depending on timing. Use a simple test HTML page served locally
or a well-known URL pattern.

- [ ] **Step 5: Run `just ci`**

- [ ] **Step 6: Commit**

```
feat(browser): auto-detect new tabs via CDP Target events
```

---

## Chunk 3: Multi-Browser + Discovery

### Task 6: Browser management actions — browsers, connect, disconnect

**Files:**

- Modify: `src/tools/browser.rs`
- Modify: `src/web/browser/manager.rs`

- [ ] **Step 1: Add action variants**

Add to the browser tool action enum: `browsers`, `connect`, `disconnect`.

New tool parameters for `connect`:
- `name` (string, required) — name for the browser
- `cdp_url` (string, required) — WebSocket URL

- [ ] **Step 2: Implement `browsers` action**

Calls `mgr.list_browsers()`. Format:

```
Known browsers:
  headless — ws://localhost:9222 (connected, 2 tabs)  [active]
  operator — ws://100.64.1.2:9222 (disconnected)
```

- [ ] **Step 3: Implement `connect` action**

Calls `mgr.connect_browser(name, Some(cdp_url))`:
1. If a browser with this name already exists, update its cdp_url and reconnect.
2. If new, create `ManagedBrowser`, insert into map.
3. Call `ensure_connected()`.
4. Set as active browser.
5. Return `BrowserInfo`.

If the connect call comes from a tool action (not config), the browser is in-memory only
until `ghost browsers add` or the `discover` action writes it to config.

- [ ] **Step 4: Implement `disconnect` action**

Calls `mgr.disconnect_browser(name)`:
1. Find browser by name.
2. Close all its tabs.
3. Drop connection (abort handler).
4. Set ConnectionState::Disconnected.
5. If this was the active browser, clear active_browser.

- [ ] **Step 5: Write live test**

Connect to a Chrome instance, verify `browsers` lists it as connected. Disconnect,
verify it shows as disconnected. Reconnect, verify it works.

- [ ] **Step 6: Run `just ci`**

- [ ] **Step 7: Commit**

```
feat(browser): add multi-browser management — browsers, connect, disconnect
```

---

### Task 7: Config write-back for browser management

**Files:**

- Modify: `src/config_cli.rs`

This enables `ghost browsers add/remove` and the `discover` action to persist browsers
to config.toml.

- [ ] **Step 1: Add `add_browser` function**

```rust
/// Add a [[web.browsers]] entry to config.toml.
///
/// If a browser with the same name exists, updates its cdp_url.
pub fn add_browser(
    name: &str,
    cdp_url: &str,
    discovered: bool,
) -> Result<(), ConfigError> {
    let path = config_path()?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| /* parse error */)?;

    // Ensure [web] table exists
    if !doc.contains_key("web") {
        doc["web"] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    // Ensure [[web.browsers]] array exists
    let web = doc["web"].as_table_mut().unwrap();
    if !web.contains_key("browsers") {
        web["browsers"] = toml_edit::Item::ArrayOfTables(
            toml_edit::ArrayOfTables::new(),
        );
    }

    let browsers = web["browsers"].as_array_of_tables_mut().unwrap();

    // Check if browser with this name already exists
    let existing = browsers.iter_mut().find(|b| {
        b.get("name").and_then(|v| v.as_str()) == Some(name)
    });

    if let Some(entry) = existing {
        entry["cdp_url"] = toml_edit::value(cdp_url);
    } else {
        let mut entry = toml_edit::Table::new();
        entry["name"] = toml_edit::value(name);
        entry["cdp_url"] = toml_edit::value(cdp_url);
        if discovered {
            entry["discovered"] = toml_edit::value(true);
        }
        browsers.push(entry);
    }

    std::fs::write(&path, doc.to_string())
        .map_err(|e| /* write error */)?;

    Ok(())
}
```

Note: This requires adding `toml_edit` as a dependency. It preserves existing formatting
and comments in config.toml, unlike deserialize→serialize round-trips.

- [ ] **Step 2: Add `remove_browser` function**

```rust
/// Remove a [[web.browsers]] entry from config.toml by name.
pub fn remove_browser(name: &str) -> Result<bool, ConfigError> {
    // Similar to add_browser, but find-and-remove from the array.
    // Return true if found and removed, false if not found.
    todo!()
}
```

- [ ] **Step 3: Add `toml_edit` dependency**

**⚠️ New dependency — requires discussion per CLAUDE.md rules.**
`toml_edit` preserves formatting and comments in config.toml, unlike
deserialize→serialize round-trips with `toml`. This is important since users
hand-edit their config files.

```bash
cargo add toml_edit
```

- [ ] **Step 4: Write unit test**

Test that `add_browser` creates a valid entry, `add_browser` with existing name updates
it, and `remove_browser` removes it. Use a temp file for config.

- [ ] **Step 5: Run `just ci`**

- [ ] **Step 6: Commit**

```
feat: add config write-back for browser management (add/remove)
```

---

### Task 8: CDP discovery + discover action

**Files:**

- Create: `src/web/browser/discovery.rs`
- Modify: `src/tools/browser.rs` (add `discover` action)

- [ ] **Step 1: Implement discovery module**

```rust
use std::net::SocketAddr;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

const CDP_PORTS: std::ops::RangeInclusive<u16> = 9222..=9229;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct DiscoveredBrowser {
    pub host: String,
    pub port: u16,
    pub cdp_url: String,
    pub browser_version: Option<String>,
}

#[derive(Deserialize)]
struct CdpVersionResponse {
    #[serde(rename = "Browser")]
    browser: Option<String>,
    #[serde(rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: Option<String>,
}

/// Discover CDP endpoints on localhost and Tailscale peers.
pub async fn discover() -> Result<Vec<DiscoveredBrowser>, BrowserError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| BrowserError::DiscoveryFailed {
            reason: e.to_string(),
        })?;

    let mut targets = Vec::new();

    // Localhost
    for port in CDP_PORTS {
        targets.push(("127.0.0.1".to_string(), port));
    }

    // Tailscale peers
    if let Ok(peers) = tailscale_peers().await {
        for ip in peers {
            for port in CDP_PORTS {
                targets.push((ip.clone(), port));
            }
        }
    }

    // Probe all targets concurrently
    let mut tasks = Vec::new();
    for (host, port) in targets {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            probe_cdp(&client, &host, port).await
        }));
    }

    let mut found = Vec::new();
    for task in tasks {
        if let Ok(Ok(Some(browser))) = task.await {
            found.push(browser);
        }
    }
    found
}

async fn probe_cdp(
    client: &Client,
    host: &str,
    port: u16,
) -> Result<Option<DiscoveredBrowser>, reqwest::Error> {
    let url = format!("http://{host}:{port}/json/version");
    let resp = client.get(&url).send().await?;
    let version: CdpVersionResponse = resp.json().await?;

    let cdp_url = version
        .web_socket_debugger_url
        .unwrap_or_else(|| format!("ws://{host}:{port}"));

    Ok(Some(DiscoveredBrowser {
        host: host.to_string(),
        port,
        cdp_url,
        browser_version: version.browser,
    }))
}

/// Get Tailscale peer IPs via `tailscale status --json`.
async fn tailscale_peers() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let output = tokio::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await?;

    if !output.status.success() {
        return Err("tailscale status failed".into());
    }

    #[derive(Deserialize)]
    struct TailscaleStatus {
        #[serde(rename = "Peer")]
        peer: Option<std::collections::HashMap<String, TailscalePeer>>,
    }
    #[derive(Deserialize)]
    struct TailscalePeer {
        #[serde(rename = "TailscaleIPs")]
        tailscale_ips: Option<Vec<String>>,
        #[serde(rename = "Online")]
        online: Option<bool>,
    }

    let status: TailscaleStatus = serde_json::from_slice(&output.stdout)?;
    let mut ips = Vec::new();
    if let Some(peers) = status.peer {
        for peer in peers.values() {
            if peer.online.unwrap_or(false) {
                if let Some(ref peer_ips) = peer.tailscale_ips {
                    // Take IPv4 addresses only
                    for ip in peer_ips {
                        if !ip.contains(':') {
                            ips.push(ip.clone());
                        }
                    }
                }
            }
        }
    }
    Ok(ips)
}
```

- [ ] **Step 2: Implement `discover` tool action**

In the browser tool handler:
1. Call `discovery::discover()`.
2. Format results:
   ```
   Discovered CDP endpoints:
     127.0.0.1:9222 — Chrome/131.0.0 (ws://127.0.0.1:9222)
     100.64.1.2:9222 — Chromium/130.0.0 (ws://100.64.1.2:9222)
   ```
3. Do NOT auto-add to config. The GHOST or operator decides whether to `connect` or
   `ghost browsers add`.

- [ ] **Step 3: Run `just ci`**

- [ ] **Step 4: Commit**

```
feat(browser): add CDP discovery — localhost + Tailscale peer scanning
```

---

## Chunk 4: CLI + Skill + Polish

### Task 9: CLI commands — ghost browsers

**Files:**

- Create: `src/cli/browsers.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Define CLI subcommands**

```rust
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum BrowsersCommand {
    /// List known browsers from config
    List,
    /// Add a browser to config.toml
    Add {
        /// Name for the browser (e.g., "headless", "operator")
        name: String,
        /// CDP WebSocket URL (e.g., ws://localhost:9222)
        cdp_url: String,
    },
    /// Remove a browser from config.toml
    Remove {
        /// Name of the browser to remove
        name: String,
    },
    /// Scan for CDP endpoints on localhost and Tailscale peers
    Discover,
    /// Test connectivity to a browser
    Check {
        /// Name of the browser to check (or "all")
        name: String,
    },
}
```

- [ ] **Step 2: Implement handlers**

- `list`: Load config, print `config.web.browsers` in a table.
- `add`: Call `config_cli::add_browser(name, cdp_url, false)`. Print success and
  remind user: "Run `ghost reboot` to apply changes."
- `remove`: Call `config_cli::remove_browser(name)`. Same reboot reminder.
- `discover`: Call `discovery::discover()`, print results. Ask user which to add
  (or print the `ghost browsers add` command for each).
- `check`: Load config, find browser by name, attempt CDP connect with 5s timeout,
  report success/failure.

- [ ] **Step 3: Wire into CLI**

Add `Browsers(BrowsersCommand)` variant to the main CLI enum. Dispatch in `main.rs`.

- [ ] **Step 4: Run `just ci`**

- [ ] **Step 5: Commit**

```
feat: add ghost browsers CLI — list, add, remove, discover, check
```

---

### Task 10: Browser-use skill file

**Files:**

- Create: `assets/skills/browser-use/skill.md`
- Modify: `src/tools/browser.rs` (slim description)

- [ ] **Step 1: Write the skill file**

```markdown
---
name: browser-use
description: How to use the browser tool — multi-browser management, tab workflow,
  operator handoff, element refs, and interaction patterns
---

# Browser Use

The `browser` tool lets you control web browsers via Chrome DevTools Protocol (CDP).
You can manage multiple browsers (headless sidecar, operator's Chrome, remote
instances) and multiple tabs per browser.

## Quick Start

1. **If browsers are pre-configured**, just use `navigate` — it auto-connects to the
   first browser and opens a tab.
2. **If no browsers are configured**, use `discover` to find CDP endpoints, or ask
   the OPERATOR to start one.

## Browser Management

### Listing and connecting

- `browsers` — list all known browsers (configured + runtime-added)
- `connect` — connect to a browser by name or CDP URL. Becomes the active browser.
- `disconnect` — disconnect from a browser, close all its tabs.
- `discover` — scan localhost and Tailscale peers for CDP endpoints.

### Active browser

All actions operate on the **active browser**. `connect` sets the active browser.
If only one browser exists, it's auto-activated on first use.

## Tab Management

### Actions

- `open` — open a new tab (optionally with a URL). Becomes the active tab. Returns
  a snapshot.
- `focus` — switch to a tab by ID. Returns a snapshot with fresh element refs.
- `close` — close a tab by ID.
- `tabs` — list open tabs in the active browser.

### Active tab

All interaction actions (navigate, snapshot, click, type, etc.) operate on the
**active tab**. `open` and `focus` change which tab is active.

### Tab limit

Maximum 5 tabs per browser. If you need more, close tabs you're done with first.

### Element refs

Element refs (e1, e2, ...) belong to the tab that produced them. When you `focus`
a different tab, the old refs are invalid — you get fresh refs from the auto-snapshot.

**Rule:** Never use refs from a snapshot of Tab A to interact with Tab B. Always
use refs from the most recent snapshot.

## Interaction Patterns

### Comparing two pages

```
1. navigate to first page → snapshot → read content
2. open second page → snapshot → read content
3. focus tab 1 to go back → snapshot → compare
```

### Following links without losing context

```
1. snapshot current page, note the ref for the link
2. open new tab (the link URL) instead of clicking
3. When done, close the new tab and focus back
```

Or just click — if the link opens in a new tab (target=_blank), it's auto-detected
and becomes the active tab.

### Form filling across pages

```
1. Tab 1: source page with data
2. Tab 2: form to fill
3. Read from tab 1, focus tab 2, fill fields
```

## Operator Handoff

When you hit a login wall, CAPTCHA, or need the OPERATOR's authenticated session:

1. **Ask the OPERATOR** to start a dedicated Chromium with CDP enabled:
   `chromium --remote-debugging-port=9222`
2. **Security:** The OPERATOR should use a separate browser (not their main one).
   Chromium with a fresh profile is ideal. CDP is unauthenticated — anyone who can
   reach the port has full browser control.
3. **Network:** The OPERATOR should expose the port via Tailscale. You can then
   `discover` their browser or `connect` directly with their Tailscale IP.
4. **Authentication:** Ask the OPERATOR to log in to the required service in that
   browser, then you can continue working in their authenticated session.
5. **When done:** `disconnect` from the operator's browser. The OPERATOR can close
   Chromium.

## Tool Actions Reference

### Browser management
| Action | Parameters | Returns |
|--------|-----------|---------|
| `browsers` | — | List of known browsers |
| `connect` | `name`, `cdp_url` | Connect + set active |
| `disconnect` | `name` | Disconnect browser |
| `discover` | — | Found CDP endpoints |

### Tab management
| Action | Parameters | Returns |
|--------|-----------|---------|
| `tabs` | `browser?` | Tab list |
| `open` | `url?` | Snapshot of new tab |
| `focus` | `tab` | Snapshot of focused tab |
| `close` | `tab` | Confirmation |

### Interaction (operates on active tab)
| Action | Key parameters |
|--------|---------------|
| `navigate` | `url` |
| `snapshot` | `offset?` |
| `click` | `ref` |
| `type` | `ref`, `text` |
| `scroll` | `direction`, `ref?` |
| `screenshot` | — |
| `press` | `key` |
| `hover` | `ref` |
| `select` | `ref`, `value` |
| `fill` | `fields` (array of [ref, value]) |
| `wait` | `ref?`, `timeout?` |
| `evaluate` | `expression` |
| `drag` | `ref`, `target_ref` |
| `upload` | `ref`, `path` |
| `resize` | `width`, `height` |
```

- [ ] **Step 2: Slim down tool description in browser.rs**

Replace the current lengthy tool description with:

```
Browser automation — navigate, read, and interact with web pages. Supports multiple
browsers and tabs. Read the browser-use skill for usage details.
```

Keep the action enum and parameter descriptions in the JSON schema — those are needed
for the LLM to know what parameters to pass. But move workflow guidance to the skill.

- [ ] **Step 3: Run `just ci`**

- [ ] **Step 4: Commit**

```
feat: add browser-use skill, slim tool description
```

---

### Task 11: Crawl4AI integration + tool output formatting

**Files:**

- Modify: `src/web/fetch.rs`
- Modify: `src/tools/browser.rs` (output formatting)

- [ ] **Step 1: Update Crawl4AI CDP URL source**

This should already be done from Task 4, Step 6. Verify that:
- When a browser is active, its CDP URL is passed to Crawl4AI.
- When no browser is active, `None` is passed and Crawl4AI uses its own browser.
- The lock on `browser_manager` is released before the async crawl4ai call.

- [ ] **Step 2: Add browser/tab context to tool output**

When the browser tool returns results, include context about which browser and tab
is active. This helps the LLM track state without calling `browsers`/`tabs` constantly.

For snapshot/navigate/click/type responses, append a status line:

```
[browser: headless | tab 2 of 3 | https://example.com]
```

For tab-changing actions (open, focus, close), include the updated tab list:

```
Opened new tab 3 in browser "headless"
[browser: headless | tab 3 of 3 (active) | about:blank]
```

- [ ] **Step 3: Run `just ci`**

- [ ] **Step 4: Run full browser live test suite**

```bash
cargo test --features live-tests browser
```

- [ ] **Step 5: Commit**

```
feat(browser): add browser/tab context to tool output, finalize Crawl4AI integration
```

---

## Final Verification

- [ ] Run `just ci` — all checks pass
- [ ] Run `cargo test --features live-tests browser` — all browser tests pass
- [ ] Manual smoke test:
  1. Start daemon with no `[[web.browsers]]` configured
  2. `ghost browsers discover` — finds local Chrome
  3. `ghost browsers add headless ws://localhost:9222`
  4. Chat with GHOST, ask it to browse a website — auto-connects, opens tab
  5. Ask it to open a second tab — multi-tab works
  6. Ask it to compare two pages — tab switching works
  7. `ghost browsers add operator ws://100.64.1.2:9222`
  8. `ghost reboot` — daemon restarts, picks up new browser
  9. GHOST can see and connect to the new browser
