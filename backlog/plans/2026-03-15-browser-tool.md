# Browser Tool Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `browser` tool that lets the GHOST control headless Chrome via CDP —
navigate, read pages via accessibility snapshots, interact with elements, take
screenshots. Share Chrome with crawl4ai for authenticated `web_fetch`.

**Architecture:** `chromiumoxide` crate connects to an external Chrome sidecar via CDP
WebSocket. Accessibility tree fetched via `Accessibility.getFullAXTree`, parsed into
`AxNode` tree, rendered as XML with ref IDs. Single tab per session, lazy connection.

**Tech Stack:** chromiumoxide (CDP), tokio, thiserror, serde_json, tracing/logfire

**Spec:** `backlog/tasks/1-user-facing/browser-tool.md` (Design Spec section)

---

## File Map

### New files

| File                               | Responsibility                                                                                                 |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `src/web/browser/mod.rs`           | `BrowserSession` struct, public API (connect, navigate, snapshot, click, type_text, scroll, screenshot, close) |
| `src/web/browser/cdp.rs`           | CDP connection via chromiumoxide, raw page commands                                                            |
| `src/web/browser/accessibility.rs` | `AxNode`, `AxRole`, `RefMap`, role classification, XML rendering                                               |
| `src/web/browser/error.rs`         | `BrowserError` enum                                                                                            |
| `src/tools/browser.rs`             | `Tool` trait impl, action dispatch, result formatting, security wrapping                                       |
| `src/web/browser/url_check.rs`     | SSRF validation for navigate URLs                                                                              |

### Modified files

| File                                         | Change                                                                      |
| -------------------------------------------- | --------------------------------------------------------------------------- |
| `src/web/browser.rs` → `src/web/crawl4ai.rs` | Rename (no content change)                                                  |
| `src/web/mod.rs`                             | Update re-exports: `browser` → `crawl4ai`, add `pub mod browser;`           |
| `src/web/fetch.rs`                           | Update `super::browser::` → `super::crawl4ai::` (3 references)              |
| `src/tools/mod.rs`                           | Add `pub mod browser;`                                                      |
| `src/tools/manager.rs`                       | Conditional browser tool registration in `for_chat()` and `all_available()` |
| `src/tools/context.rs`                       | Add `browser_session` field to `ToolContext`                                |
| `src/config.rs`                              | Add `chrome_cdp_url` to `WebSettings` and `WebConfig`                       |
| `src/web/crawl4ai.rs`                        | Pass `cdp_url` to crawl4ai `BrowserConfig` when available                   |
| `Cargo.toml`                                 | Add `chromiumoxide` dependency                                              |
| `docker-compose.yml`                         | Add Chrome sidecar service                                                  |

---

## Task 1: Rename `browser.rs` → `crawl4ai.rs`

Do the rename first to avoid confusion with the new `browser/` module.

**Files:**

- Rename: `src/web/browser.rs` → `src/web/crawl4ai.rs`
- Modify: `src/web/mod.rs`
- Modify: `src/web/fetch.rs`

- [ ] **Step 1: Rename the file**

```bash
git mv src/web/browser.rs src/web/crawl4ai.rs
```

- [ ] **Step 2: Update `src/web/mod.rs`**

Change `mod browser;` to `mod crawl4ai;` and update the re-export line:

```rust
// Old:
pub use browser::{Crawl4aiOptions, fetch_with_crawl4ai};
// New:
pub use crawl4ai::{Crawl4aiOptions, fetch_with_crawl4ai};
```

- [ ] **Step 3: Update `src/web/fetch.rs`**

Three references to change (lines ~80, ~137, ~140):

```rust
// Old:
super::browser::Crawl4aiOptions
super::browser::fetch_with_crawl4ai
// New:
super::crawl4ai::Crawl4aiOptions
super::crawl4ai::fetch_with_crawl4ai
```

- [ ] **Step 4: Verify it compiles**

```bash
cargo check
```

- [ ] **Step 5: Run tests**

```bash
cargo test
```

- [ ] **Step 6: Commit**

```bash
git add src/web/browser.rs src/web/crawl4ai.rs src/web/mod.rs src/web/fetch.rs
git commit -m "refactor: rename web/browser.rs to web/crawl4ai.rs

Clears the 'browser' name for the upcoming browser tool module."
```

---

## Task 2: Config — add `chrome_cdp_url`

**Files:**

- Modify: `src/config.rs`

- [ ] **Step 1: Add to `WebSettings`** (line ~146)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub search_max_results: Option<usize>,
    pub crawl4ai_url: Option<String>,
    pub chrome_cdp_url: Option<String>,    // NEW
    pub search: Option<SearchProviderSettings>,
}
```

- [ ] **Step 2: Add to `WebConfig`** (line ~252)

```rust
#[derive(Debug, Clone, Serialize)]
pub struct WebConfig {
    pub search_max_results: usize,
    pub crawl4ai_url: Option<String>,
    pub chrome_cdp_url: Option<String>,    // NEW
    pub search_provider: SearchProviderConfig,
}
```

- [ ] **Step 3: Add resolution in `Config::from_settings()`** (line ~428)

In the `web:` block, after the `crawl4ai_url` resolution, add:

```rust
let chrome_cdp_url = settings
    .web
    .as_ref()
    .and_then(|w| w.chrome_cdp_url.clone())
    .or_else(|| env::var("CHROME_CDP_URL").ok());
```

And add `chrome_cdp_url,` to the `WebConfig { ... }` construction.

- [ ] **Step 4: Update `test_config()` helpers**

Add `chrome_cdp_url: None` to `WebConfig` construction in `src/config.rs`
`test_config()` (~line 621) and `tests/common.rs` `test_config()`.

- [ ] **Step 5: Verify**

```bash
cargo check && cargo test
```

- [ ] **Step 6: Commit**

```bash
git add src/config.rs tests/common.rs
git commit -m "feat: add chrome_cdp_url to web config"
```

---

## Task 3: `BrowserError` enum

**Files:**

- Create: `src/web/browser/error.rs`
- Create: `src/web/browser/mod.rs` (minimal barrel, just `pub mod error;`)
- Modify: `src/web/mod.rs` (add `pub mod browser;`)

- [ ] **Step 1: Create `src/web/browser/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("browser not connected — is Chrome running at {url}?")]
    ConnectionFailed {
        url: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("navigation to {url} failed: {reason}")]
    NavigationFailed { url: String, reason: String },

    #[error("navigation to {url} timed out after {timeout_secs}s")]
    NavigationTimeout { url: String, timeout_secs: u64 },

    #[error("element [ref={ref_id}] not found — page may have changed, try 'snapshot'")]
    RefNotFound { ref_id: String },

    #[error("element [ref={ref_id}] is not interactable: {reason}")]
    NotInteractable { ref_id: String, reason: String },

    #[error("screenshot failed: {reason}")]
    ScreenshotFailed { reason: String },

    #[error("CDP error: {message}")]
    CdpError { message: String },

    #[error("URL not allowed: {reason}")]
    UrlBlocked { reason: String },
}
```

- [ ] **Step 2: Create `src/web/browser/mod.rs`**

```rust
pub mod error;

pub use error::BrowserError;
```

- [ ] **Step 3: Add `pub mod browser;` to `src/web/mod.rs`**

- [ ] **Step 4: Verify**

```bash
cargo check
```

- [ ] **Step 5: Commit**

```bash
git add src/web/browser/
git commit -m "feat: add BrowserError enum for browser tool"
```

---

## Task 4: Accessibility tree — `AxNode`, role classification, XML rendering

This is the core of the browser tool. Build it with unit tests — no CDP dependency.

**Files:**

- Create: `src/web/browser/accessibility.rs`
- Modify: `src/web/browser/mod.rs`

- [ ] **Step 1: Write tests for role classification**

At the bottom of `accessibility.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_roles_get_refs() {
        assert_eq!(classify_role("button"), RoleClass::Interactive);
        assert_eq!(classify_role("textbox"), RoleClass::Interactive);
        assert_eq!(classify_role("link"), RoleClass::Interactive);
    }

    #[test]
    fn content_roles_get_refs_when_named() {
        assert_eq!(classify_role("heading"), RoleClass::Content);
        assert_eq!(classify_role("img"), RoleClass::Content);
    }

    #[test]
    fn structural_roles_never_get_refs() {
        assert_eq!(classify_role("list"), RoleClass::Structural);
        assert_eq!(classify_role("navigation"), RoleClass::Structural);
        assert_eq!(classify_role("main"), RoleClass::Structural);
    }

    #[test]
    fn unknown_roles_treated_as_structural() {
        assert_eq!(classify_role("banana"), RoleClass::Structural);
    }
}
```

- [ ] **Step 2: Implement `AxRole`, `RoleClass`, `classify_role`**

```rust
/// Classification that determines whether a node gets a ref ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleClass {
    /// Always assigned a ref. Elements the GHOST can act on (click, type).
    Interactive,
    /// Assigned a ref only when the node has a non-empty name.
    Content,
    /// Never assigned a ref. Provides hierarchy and grouping.
    Structural,
}

/// Classify an accessibility role string into its ref-assignment category.
///
/// - **Interactive**: button, checkbox, combobox, link, listbox, menuitem,
///   menuitemcheckbox, menuitemradio, option, radio, searchbox, slider,
///   spinbutton, switch, tab, textbox, treeitem
/// - **Content**: cell, columnheader, heading, img, listitem, rowheader
/// - **Structural**: everything else (list, navigation, main, form, ...)
pub fn classify_role(role: &str) -> RoleClass {
    match role {
        "button" | "checkbox" | "combobox" | "link" | "listbox" | "menuitem"
        | "menuitemcheckbox" | "menuitemradio" | "option" | "radio" | "searchbox"
        | "slider" | "spinbutton" | "switch" | "tab" | "textbox" | "treeitem" => {
            RoleClass::Interactive
        }
        "cell" | "columnheader" | "heading" | "img" | "listitem" | "rowheader" => {
            RoleClass::Content
        }
        _ => RoleClass::Structural,
    }
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p ghost web::browser::accessibility
```

Expected: all 4 tests pass.

- [ ] **Step 4: Write tests for ref assignment**

```rust
    #[test]
    fn ref_map_assigns_sequential_ids() {
        let mut refs = RefMap::new();
        let r1 = refs.assign(100);
        let r2 = refs.assign(200);
        assert_eq!(r1, "e1");
        assert_eq!(r2, "e2");
    }

    #[test]
    fn ref_map_resolves_ids() {
        let mut refs = RefMap::new();
        refs.assign(42);
        assert_eq!(refs.resolve("e1"), Some(42));
        assert_eq!(refs.resolve("e99"), None);
    }

    #[test]
    fn ref_map_reset_clears_and_restarts() {
        let mut refs = RefMap::new();
        refs.assign(1);
        refs.reset();
        let r = refs.assign(2);
        assert_eq!(r, "e1");
        assert_eq!(refs.resolve("e1"), Some(2));
    }
```

- [ ] **Step 5: Implement `RefMap`**

```rust
use std::collections::HashMap;

/// Maps ref IDs ("e1", "e2", ...) to Chrome BackendDOMNodeIds.
///
/// Refs are sequential, assigned in depth-first tree order during snapshot.
/// The map is **invalidated on every `snapshot()` call** — `reset()` clears
/// all refs and restarts the counter from 1.
///
/// Between snapshots, refs are stable: "e5" always points to the same DOM
/// node. If the page mutates without a new snapshot, refs may be stale.
pub struct RefMap {
    refs: HashMap<String, i64>,
    counter: u32,
}

impl RefMap {
    pub fn new() -> Self {
        Self {
            refs: HashMap::new(),
            counter: 0,
        }
    }

    /// Assign the next ref ID to a backend node ID. Returns the ref string.
    pub fn assign(&mut self, backend_node_id: i64) -> String {
        self.counter += 1;
        let ref_id = format!("e{}", self.counter);
        self.refs.insert(ref_id.clone(), backend_node_id);
        ref_id
    }

    /// Look up the backend node ID for a ref string (e.g. "e5").
    pub fn resolve(&self, ref_id: &str) -> Option<i64> {
        self.refs.get(ref_id).copied()
    }

    /// Clear all refs and restart counter. Called at the start of each snapshot.
    pub fn reset(&mut self) {
        self.refs.clear();
        self.counter = 0;
    }
}
```

- [ ] **Step 6: Run tests, verify pass**

- [ ] **Step 7: Write tests for XML rendering**

```rust
    #[test]
    fn render_simple_tree() {
        let tree = vec![
            AxNode {
                role: "heading".into(),
                name: "Hello".into(),
                backend_node_id: Some(1),
                properties: AxProperties { level: Some(1), ..Default::default() },
                children: vec![],
            },
            AxNode {
                role: "button".into(),
                name: "Click me".into(),
                backend_node_id: Some(2),
                properties: AxProperties::default(),
                children: vec![],
            },
        ];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains(r#"<heading level="1" ref="e1">Hello</heading>"#));
        assert!(xml.contains(r#"<button ref="e2">Click me</button>"#));
    }

    #[test]
    fn render_nested_structure() {
        let tree = vec![AxNode {
            role: "navigation".into(),
            name: "Main".into(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![
                AxNode {
                    role: "link".into(),
                    name: "Home".into(),
                    backend_node_id: Some(10),
                    properties: AxProperties::default(),
                    children: vec![],
                },
            ],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains(r#"<navigation name="Main">"#));
        assert!(xml.contains(r#"  <link ref="e1">Home</link>"#));
        assert!(xml.contains("</navigation>"));
    }

    #[test]
    fn structural_nodes_get_no_ref() {
        let tree = vec![AxNode {
            role: "list".into(),
            name: String::new(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(!xml.contains("ref="));
    }

    #[test]
    fn text_nodes_render_without_ref() {
        let tree = vec![AxNode {
            role: "StaticText".into(),
            name: "Hello world".into(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains("<text>Hello world</text>"));
        assert!(!xml.contains("ref="));
    }

    #[test]
    fn xml_escapes_special_chars() {
        let tree = vec![AxNode {
            role: "button".into(),
            name: "A < B & C".into(),
            backend_node_id: Some(1),
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains("A &lt; B &amp; C"));
    }

    #[test]
    fn truncation_at_node_limit() {
        let tree: Vec<AxNode> = (0..10)
            .map(|i| AxNode {
                role: "button".into(),
                name: format!("Button {i}"),
                backend_node_id: Some(i as i64),
                properties: AxProperties::default(),
                children: vec![],
            })
            .collect();
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 5, 15, 0);
        assert!(xml.contains("Button 0"));
        assert!(xml.contains("Button 4"));
        assert!(!xml.contains("Button 5"));
        assert!(xml.contains("<!-- Snapshot truncated:"));
    }
```

- [ ] **Step 8: Implement `AxNode`, `AxProperties`, `render_xml`**

```rust
/// A node in the parsed accessibility tree.
///
/// Built from Chrome's `Accessibility.getFullAXTree` CDP response.
/// Ignored nodes and empty anonymous containers are pruned.
#[derive(Debug, Clone)]
pub struct AxNode {
    pub role: String,
    pub name: String,
    pub backend_node_id: Option<i64>,
    pub properties: AxProperties,
    pub children: Vec<AxNode>,
}

#[derive(Debug, Clone, Default)]
pub struct AxProperties {
    pub level: Option<u32>,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub expanded: Option<bool>,
}

/// Render a list of root AxNodes as XML text with ref assignment.
///
/// - Nodes are rendered as XML elements named after their role
/// - Interactive nodes always get a ref attribute
/// - Content nodes get a ref only when they have a non-empty name
/// - Structural nodes never get a ref
/// - StaticText nodes render as `<text>content</text>` (no ref)
/// - `max_nodes`: stop rendering after this many nodes (append truncation comment)
/// - `max_depth`: omit nodes deeper than this level
pub fn render_xml(
    roots: &[AxNode],
    refs: &mut RefMap,
    max_nodes: usize,
    max_depth: usize,
    offset: usize,
) -> String {
    // Implementation: depth-first walk, build XML string with indentation,
    // assign refs per role classification, escape XML special chars.
    //
    // Node counting: ALL nodes (interactive, content, structural) count toward
    // the limit. `offset` skips the first N nodes in depth-first order (refs
    // are still assigned to skipped nodes so numbering stays consistent).
    // After skipping `offset` nodes, render up to `max_nodes` nodes.
    // When truncated, append:
    // <!-- Snapshot truncated: showing {max_nodes} of {total} nodes. Use offset={offset+max_nodes} to see more. -->
    // ...
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
```

Full `render_xml` implementation: walk depth-first, emit opening tag with attributes
(ref, level, value, checked, expanded, name on structural nodes), recurse children with
indent+2, emit closing tag. For `StaticText` role, emit `<text>` instead. Track node
count; when limit reached, append
`<!-- Snapshot truncated: showing N of M nodes. Use offset=N to see more. -->`.

- [ ] **Step 9: Run all tests, verify pass**

```bash
cargo test -p ghost web::browser::accessibility
```

- [ ] **Step 10: Commit**

```bash
git add src/web/browser/accessibility.rs src/web/browser/mod.rs
git commit -m "feat: accessibility tree parsing, ref assignment, and XML rendering"
```

---

## Task 5: CDP connection and page actions

**Files:**

- Create: `src/web/browser/cdp.rs`
- Modify: `src/web/browser/mod.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `chromiumoxide` to `Cargo.toml`**

```toml
chromiumoxide = { version = "0.7", features = ["tokio-runtime"], default-features = false }
```

Run `cargo check` to verify it resolves.

- [ ] **Step 2: Implement `cdp.rs`**

```rust
use chromiumoxide::Browser;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::accessibility;
use chromiumoxide::cdp::browser_protocol::dom;

use super::error::BrowserError;

const NAVIGATION_TIMEOUT_SECS: u64 = 30;
const VIEWPORT_WIDTH: u32 = 1280;
const VIEWPORT_HEIGHT: u32 = 720;

/// Connect to Chrome via CDP WebSocket URL.
/// Returns (Browser, event_loop_handle).
pub async fn connect(cdp_url: &str) -> Result<(Browser, impl Future), BrowserError> {
    // Use chromiumoxide::Browser::connect(cdp_url)
    // Map errors to BrowserError::ConnectionFailed
}

/// Open a new tab with the default viewport size.
pub async fn new_page(browser: &Browser) -> Result<Page, BrowserError> {
    // browser.new_page("about:blank")
    // Set viewport to VIEWPORT_WIDTH x VIEWPORT_HEIGHT
}

/// Navigate to URL and wait for page load.
pub async fn navigate(page: &Page, url: &str) -> Result<(String, String), BrowserError> {
    // page.goto(url) with timeout
    // Return (final_url, title)
}

/// Fetch the full accessibility tree as raw CDP response.
pub async fn get_accessibility_tree(
    page: &Page,
) -> Result<Vec<accessibility::AXNode>, BrowserError> {
    // page.execute(Accessibility::getFullAXTree {})
    // Return the nodes array
}

/// Click a DOM node by BackendNodeId.
pub async fn click_node(page: &Page, backend_node_id: i64) -> Result<(), BrowserError> {
    // Resolve node to coordinates via DOM.getBoxModel or DOM.resolveNode
    // Then Page.dispatchMouseEvent (or use chromiumoxide click helpers)
}

/// Type text into a focused element by BackendNodeId.
pub async fn type_into_node(
    page: &Page,
    backend_node_id: i64,
    text: &str,
) -> Result<(), BrowserError> {
    // Focus node via DOM.focus
    // Input.dispatchKeyEvent for each character (or Input.insertText)
}

/// Scroll the page or a specific element.
pub async fn scroll(
    page: &Page,
    direction: ScrollDirection,
    backend_node_id: Option<i64>,
) -> Result<(), BrowserError> {
    // If backend_node_id: scroll element into view via DOM.scrollIntoViewIfNeeded
    // Else: page.evaluate("window.scrollBy(0, delta)")
}

/// Capture page screenshot as PNG bytes.
pub async fn screenshot(page: &Page) -> Result<Vec<u8>, BrowserError> {
    // page.screenshot(ScreenshotParams::default())
}

pub enum ScrollDirection {
    Up,
    Down,
}
```

This file wraps all `chromiumoxide` calls. The exact chromiumoxide API calls will need
to be adapted to the crate's actual API — consult chromiumoxide docs via context7 MCP
during implementation.

**Risk**: `click_node` and `type_into_node` using `BackendNodeId` may not work with
chromiumoxide's high-level helpers (which expect `Element` from CSS selectors). You'll
likely need raw CDP commands: `DOM.resolveNode` → `DOM.getBoxModel` →
`Input.dispatchMouseEvent` for clicks, and `DOM.focus` → `Input.insertText` for typing.
chromiumoxide supports `page.execute(CdpCommand)` for arbitrary CDP calls.

- [ ] **Step 3: Verify it compiles**

```bash
cargo check
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/web/browser/cdp.rs src/web/browser/mod.rs
git commit -m "feat: CDP connection and page action primitives"
```

---

## Task 6: `BrowserSession` — public API

**Files:**

- Modify: `src/web/browser/mod.rs`

- [ ] **Step 1: Implement `BrowserSession`**

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use super::accessibility::{self, AxNode, RefMap, render_xml, parse_ax_tree};
use super::cdp;
use super::error::BrowserError;

const MAX_SNAPSHOT_NODES: usize = 500;
const MAX_SNAPSHOT_DEPTH: usize = 15;

/// A browser session connected to Chrome via CDP.
///
/// Manages a single tab, maintains ref map between snapshots.
/// Created lazily on first `browser` tool call, dropped on session end.
pub struct BrowserSession {
    page: chromiumoxide::Page,
    refs: RefMap,
    cdp_url: String,
    // Keep browser handle alive (dropped = disconnect)
    _browser: chromiumoxide::Browser,
}

impl BrowserSession {
    /// Connect to Chrome at the given CDP WebSocket URL and open a tab.
    pub async fn connect(cdp_url: &str) -> Result<Self, BrowserError> {
        let (browser, handler) = cdp::connect(cdp_url).await?;
        // Spawn the CDP event loop. Store the handle so we can abort on drop.
        let handler_handle = tokio::spawn(handler);
        let page = cdp::new_page(&browser).await?;
        Ok(Self {
            page,
            refs: RefMap::new(),
            cdp_url: cdp_url.to_string(),
            _browser: browser,
            _handler: handler_handle,
        })
    }

    // Add `_handler: tokio::task::JoinHandle<()>` to the struct fields.
    // Implement Drop to abort the handler task:
    // impl Drop for BrowserSession {
    //     fn drop(&mut self) { self.handler.abort(); }
    // }

    /// Navigate to a URL. Returns (final_url, title).
    pub async fn navigate(&mut self, url: &str) -> Result<(String, String), BrowserError> {
        self.refs.reset();
        cdp::navigate(&self.page, url).await
    }

    /// Get the current page's accessibility tree as XML with fresh refs.
    pub async fn snapshot(&mut self, offset: usize) -> Result<String, BrowserError> {
        self.refs.reset();
        let raw_nodes = cdp::get_accessibility_tree(&self.page).await?;
        let tree = parse_ax_tree(&raw_nodes);
        // If offset > 0, skip first `offset` nodes during rendering
        let xml = render_xml(&tree, &mut self.refs, MAX_SNAPSHOT_NODES, MAX_SNAPSHOT_DEPTH);
        Ok(xml)
    }

    /// Click an element by ref ID.
    pub async fn click(&self, ref_id: &str) -> Result<String, BrowserError> {
        let node_id = self.refs.resolve(ref_id).ok_or_else(|| {
            BrowserError::RefNotFound { ref_id: ref_id.to_string() }
        })?;
        cdp::click_node(&self.page, node_id).await?;
        // Return description of what was clicked (from ref's role+name)
        Ok(format!("Clicked [ref={ref_id}]"))
    }

    /// Type text into an element by ref ID.
    pub async fn type_text(&self, ref_id: &str, text: &str) -> Result<String, BrowserError> {
        let node_id = self.refs.resolve(ref_id).ok_or_else(|| {
            BrowserError::RefNotFound { ref_id: ref_id.to_string() }
        })?;
        cdp::type_into_node(&self.page, node_id, text).await?;
        Ok(format!("Typed into [ref={ref_id}]"))
    }

    /// Scroll the page or a specific element.
    pub async fn scroll(
        &self,
        direction: cdp::ScrollDirection,
        ref_id: Option<&str>,
    ) -> Result<String, BrowserError> {
        let node_id = ref_id
            .map(|r| self.refs.resolve(r).ok_or_else(|| {
                BrowserError::RefNotFound { ref_id: r.to_string() }
            }))
            .transpose()?;
        cdp::scroll(&self.page, direction, node_id).await?;
        let dir = match direction {
            cdp::ScrollDirection::Up => "up",
            cdp::ScrollDirection::Down => "down",
        };
        Ok(format!("Scrolled {dir}"))
    }

    /// Capture screenshot, save to workspace, return file path.
    pub async fn screenshot(&self, workspace: &Path) -> Result<PathBuf, BrowserError> {
        let bytes = cdp::screenshot(&self.page).await?;
        let dir = workspace.join(".cache/browser");
        tokio::fs::create_dir_all(&dir).await.map_err(|e| {
            BrowserError::ScreenshotFailed { reason: e.to_string() }
        })?;
        let filename = format!(
            "screenshot-{}.png",
            chrono::Utc::now().format("%Y-%m-%d-%H%M%S")
        );
        let path = dir.join(&filename);
        tokio::fs::write(&path, &bytes).await.map_err(|e| {
            BrowserError::ScreenshotFailed { reason: e.to_string() }
        })?;
        Ok(path)
    }

    /// Get the current page URL.
    pub async fn current_url(&self) -> Result<String, BrowserError> {
        // page.url() or page.evaluate("location.href")
        todo!()
    }

    /// Get the current page title.
    pub async fn current_title(&self) -> Result<String, BrowserError> {
        // page.evaluate("document.title")
        todo!()
    }
}
```

- [ ] **Step 2: Add `parse_ax_tree` to `accessibility.rs`**

This function converts Chrome's raw `AXNode` array into our `AxNode` tree:

```rust
/// Parse Chrome's flat AXNode array into a tree of AxNode.
///
/// Chrome returns nodes in a flat array with `childIds` references.
/// We build a HashMap of nodeId → node, then reconstruct the tree
/// starting from the root (first node). Ignored nodes are skipped.
pub fn parse_ax_tree(raw_nodes: &[chromiumoxide::cdp::...::AXNode]) -> Vec<AxNode> {
    // Build id→node map, reconstruct tree from childIds, skip ignored
}
```

- [ ] **Step 3: Export new public items from `mod.rs`**

```rust
pub mod accessibility;
pub mod cdp;
pub mod error;

pub use error::BrowserError;
// BrowserSession is the main public type
```

- [ ] **Step 4: Verify it compiles**

```bash
cargo check
```

- [ ] **Step 5: Commit**

```bash
git add src/web/browser/
git commit -m "feat: BrowserSession with navigate, snapshot, click, type, scroll, screenshot"
```

---

## Task 7: SSRF protection

**Files:**

- Create: `src/web/browser/url_check.rs` (or inline in `mod.rs` if small)

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_http_urls() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn blocks_non_http_schemes() {
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("data:text/html,<h1>hi</h1>").is_err());
    }

    #[test]
    fn blocks_private_ips() {
        assert!(validate_url("http://127.0.0.1").is_err());
        assert!(validate_url("http://10.0.0.1").is_err());
        assert!(validate_url("http://192.168.1.1").is_err());
        assert!(validate_url("http://172.16.0.1").is_err());
    }

    #[test]
    fn blocks_localhost() {
        assert!(validate_url("http://localhost").is_err());
        assert!(validate_url("http://localhost:9222").is_err());
    }
}
```

- [ ] **Step 2: Implement `validate_url`**

```rust
use std::net::IpAddr;
use url::Url;

/// Validate that a URL is safe to navigate to.
///
/// Blocks:
/// - Non-http(s) schemes (file:, javascript:, data:)
/// - Private/reserved IP ranges (127/8, 10/8, 172.16/12, 192.168/16, 169.254/16)
/// - localhost
pub fn validate_url(raw: &str) -> Result<Url, BrowserError> {
    let url = Url::parse(raw).map_err(|e| BrowserError::UrlBlocked {
        reason: format!("invalid URL: {e}"),
    })?;

    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(BrowserError::UrlBlocked {
            reason: format!("scheme '{scheme}' not allowed, use http or https"),
        }),
    }

    if let Some(host) = url.host_str() {
        if host == "localhost" {
            return Err(BrowserError::UrlBlocked {
                reason: "localhost not allowed".into(),
            });
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_private_ip(ip) {
                return Err(BrowserError::UrlBlocked {
                    reason: format!("private IP {ip} not allowed"),
                });
            }
        }
    }

    Ok(url)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}
```

Note: `url` crate is already in `Cargo.toml`.

**DNS rebinding**: The spec calls for resolving DNS before connecting to catch hostnames
that resolve to private IPs. This requires async DNS resolution
(`tokio::net::lookup_host`). For MVP, the IP check on the parsed URL is sufficient — DNS
rebinding is a lower-priority threat (attacker needs to control DNS for a domain the
GHOST is navigating to). Add DNS resolution as a follow-up hardening task.

- [ ] **Step 3: Run tests**

```bash
cargo test -p ghost web::browser::url_check
```

- [ ] **Step 4: Add `pub mod url_check;` to `src/web/browser/mod.rs`**

- [ ] **Step 5: Wire into `BrowserSession::navigate`**

Call `url_check::validate_url(url)?` at the top of `navigate()`.

- [ ] **Step 6: Commit**

```bash
git add src/web/browser/
git commit -m "feat: SSRF protection for browser navigation"
```

---

## Task 8: `ToolContext` integration

**Files:**

- Modify: `src/tools/context.rs`

- [ ] **Step 1: Add `browser_session` field to `ToolContext`**

Use `Arc<tokio::sync::Mutex<Option<BrowserSession>>>` — NOT `Option<Arc<...>>`. The
`Arc` is always present so the tool can lock and populate the inner `Option` through
`&ToolContext` (which is immutable). Cloning `ToolContext` clones the `Arc` (cheap, same
underlying session).

```rust
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

pub struct ToolContext {
    // ... existing fields ...
    pub browser_session: Arc<TokioMutex<Option<BrowserSession>>>,
}
```

Remove `#[derive(Debug)]` from `ToolContext` and implement `Debug` manually —
`BrowserSession` wraps chromiumoxide types that don't implement `Debug`:

```rust
impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("workspace", &self.workspace)
            .field("session_id", &self.session_id)
            .field("browser_session", &"<browser>")
            .finish_non_exhaustive()
    }
}
```

- [ ] **Step 2: Update all `ToolContext` construction sites**

Search for `ToolContext {` in the codebase. Add
`browser_session: Arc::new(TokioMutex::new(None))` to each. Known sites:
`src/chat/session.rs`, `src/scripting/bindings.rs`, `src/tools/shell.rs`,
`src/tools/file_edit.rs`, `src/tools/write_file.rs`, `src/tools/read_file.rs`,
`tests/tools.rs`, `tests/knowledge.rs`, `tests/coding_agent.rs`, `tests/common.rs`
(`test_config()`).

```bash
grep -rn "ToolContext {" src/ tests/
```

- [ ] **Step 3: Verify**

```bash
cargo check
```

- [ ] **Step 4: Commit**

```bash
git add src/tools/context.rs src/chat/ src/agents/ tests/
git commit -m "feat: add browser_session field to ToolContext"
```

---

## Task 9: Browser tool — `Tool` trait implementation

**Files:**

- Create: `src/tools/browser.rs`
- Modify: `src/tools/mod.rs`
- Modify: `src/tools/manager.rs`

- [ ] **Step 1: Create `src/tools/browser.rs`**

```rust
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::providers::ToolDefinition;
use crate::web::browser::{BrowserSession, BrowserError};
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
                reads.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["navigate", "snapshot", "click", "type", "scroll", "screenshot"],
                        "description": "navigate: open URL (replaces current page). \
                            snapshot: get accessibility tree with ref IDs. \
                            click: click element by ref. type: enter text into element \
                            by ref. scroll: scroll page or element. screenshot: capture \
                            page as PNG image."
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to. Required for 'navigate'."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Element ref ID (e.g. 'e5'). Required for 'click' \
                            and 'type'. Optional for 'scroll' (scrolls element into view)."
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to type. Required for 'type'."
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down"],
                        "description": "Scroll direction. Defaults to 'down'. Only for 'scroll'."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Skip first N nodes in snapshot. For paginating \
                            large trees. Only for 'snapshot'."
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = params.get("action").and_then(Value::as_str).ok_or_else(|| {
            ToolError::InvalidParams("missing required parameter: action".into())
        })?;

        // Lazy-init: lock the Arc<Mutex<Option<BrowserSession>>>, create if None
        let mut guard = ctx.browser_session.lock().await;
        if guard.is_none() {
            let cdp_url = ctx.config.web.chrome_cdp_url.as_deref().ok_or_else(|| {
                ToolError::ExecutionFailed("chrome_cdp_url not configured".into())
            })?;
            let session = BrowserSession::connect(cdp_url).await.map_err(|e| {
                ToolError::ExecutionFailed(e.to_string())
            })?;
            *guard = Some(session);
        }
        let session = guard.as_mut().unwrap();

        match action {
            "navigate" => {
                let url = params.get("url").and_then(Value::as_str).ok_or_else(|| {
                    ToolError::InvalidParams("'navigate' requires 'url' parameter".into())
                })?;
                let (final_url, title) = session.navigate(url).await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let result = json!({"ok": true, "url": final_url, "title": title});
                Ok(ToolOutput::text(result.to_string()))
            }
            "snapshot" => {
                let offset = params.get("offset")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                let xml = session.snapshot(offset).await
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
            "click" => {
                let ref_id = params.get("ref").and_then(Value::as_str).ok_or_else(|| {
                    ToolError::InvalidParams("'click' requires 'ref' parameter".into())
                })?;
                let desc = session.click(ref_id).await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let url = session.current_url().await.unwrap_or_default();
                let result = json!({"ok": true, "url": url, "description": desc});
                Ok(ToolOutput::text(result.to_string()))
            }
            "type" => {
                let ref_id = params.get("ref").and_then(Value::as_str).ok_or_else(|| {
                    ToolError::InvalidParams("'type' requires 'ref' parameter".into())
                })?;
                let text = params.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ToolError::InvalidParams("'type' requires 'text' parameter".into())
                })?;
                let desc = session.type_text(ref_id, text).await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let url = session.current_url().await.unwrap_or_default();
                let result = json!({"ok": true, "url": url, "description": desc});
                Ok(ToolOutput::text(result.to_string()))
            }
            "scroll" => {
                let direction = match params.get("direction").and_then(Value::as_str) {
                    Some("up") => ScrollDirection::Up,
                    _ => ScrollDirection::Down,
                };
                let ref_id = params.get("ref").and_then(Value::as_str);
                let desc = session.scroll(direction, ref_id).await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let url = session.current_url().await.unwrap_or_default();
                let result = json!({"ok": true, "url": url, "description": desc});
                Ok(ToolOutput::text(result.to_string()))
            }
            "screenshot" => {
                let path = session.screenshot(&ctx.workspace).await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                // Relative path for display
                let rel = path.strip_prefix(&ctx.workspace).unwrap_or(&path);
                let result = json!({
                    "ok": true,
                    "url": session.current_url().await.unwrap_or_default(),
                    "path": rel.display().to_string(),
                    "description": format!("Screenshot captured (1280x720)")
                });
                Ok(ToolOutput::text(result.to_string()))
            }
            _ => Err(ToolError::InvalidParams(format!("unknown action: {action}"))),
        }
    }
}
```

**Session persistence**: uses `Arc<tokio::sync::Mutex<Option<BrowserSession>>>` on
`ToolContext` (see Task 8). The tool locks the mutex, checks if `None`, creates the
session if needed, stores it back. Works through `&ToolContext` via interior mutability.

- [ ] **Step 2: Add `pub mod browser;` to `src/tools/mod.rs`**

- [ ] **Step 3: Register in `ToolManager`**

In `src/tools/manager.rs`, modify `for_chat()` and `all_available()`. Since these
methods don't take `Config`, use a separate method:

```rust
/// Conditionally register the browser tool if Chrome CDP is configured.
pub fn with_browser_if_configured(&mut self, config: &Config) {
    if config.web.chrome_cdp_url.is_some() {
        self.register(Arc::new(super::browser::BrowserTool));
    }
}
```

Call sites to update:

- `src/chat/session.rs` `SessionChat::from_config()`: after `ToolManager::for_chat()`,
  call `manager.with_browser_if_configured(&config)` before passing to `Self::new()`
- `src/tools/manager.rs` `all_available()`: call `with_browser_if_configured` at the end
  (but `all_available()` doesn't take config — either add a `Config` parameter or add
  the browser tool unconditionally in `all_available` since it's used for agent tool
  whitelisting and the tool itself checks config at runtime)

- [ ] **Step 4: Verify**

```bash
cargo check
```

- [ ] **Step 5: Commit**

```bash
git add src/tools/browser.rs src/tools/mod.rs src/tools/manager.rs
git commit -m "feat: browser tool implementation with action dispatch"
```

---

## Task 10: crawl4ai shared Chrome integration

**Files:**

- Modify: `src/web/crawl4ai.rs`

- [ ] **Step 1: Pass `cdp_url` to crawl4ai requests**

In `fetch_with_crawl4ai`, add an optional `cdp_url` parameter:

```rust
pub async fn fetch_with_crawl4ai(
    base_url: &str,
    page_url: &str,
    options: &Crawl4aiOptions,
    cdp_url: Option<&str>,   // NEW
) -> Result<String, WebError> {
```

In the request body, add `cdp_url` to `browser_config.params` when present:

```rust
let mut browser_params = json!({ "headless": true });
if let Some(url) = cdp_url {
    browser_params["cdp_url"] = json!(url);
}
let body = json!({
    "urls": [page_url],
    "browser_config": {
        "type": "BrowserConfig",
        "params": browser_params
    },
    // ...
});
```

- [ ] **Step 2: Update call site in `src/web/fetch.rs`**

Pass `chrome_cdp_url` through from `FetchOptions` or add it as a parameter.

- [ ] **Step 3: Verify existing web_fetch still works**

```bash
cargo test -p ghost web
```

- [ ] **Step 4: Commit**

```bash
git add src/web/crawl4ai.rs src/web/fetch.rs
git commit -m "feat: pass chrome_cdp_url to crawl4ai for shared browser"
```

---

## Task 11: Docker compose

**Files:**

- Modify: `docker-compose.yml`

- [ ] **Step 1: Add Chrome sidecar**

```yaml
chrome:
  image: chromedp/headless-shell:stable
  ports:
    - "127.0.0.1:9222:9222"
  shm_size: "2gb"
  init: true
  restart: unless-stopped
  deploy:
    resources:
      limits:
        memory: 2g
        cpus: "1.0"
```

- [ ] **Step 2: Commit**

```bash
git add docker-compose.yml
git commit -m "feat: add Chrome headless sidecar for browser tool"
```

---

## Task 12: Integration test (requires running Chrome)

**Files:**

- Create: test in `tests/` or as a live test in an existing test file

- [ ] **Step 1: Write integration test**

This test requires Chrome running at `ws://localhost:9222` (use `live-tests` feature
flag). Read the @testing skill for test conventions.

```rust
#[tokio::test]
#[cfg(feature = "live-tests")]
async fn browser_navigate_and_snapshot() {
    let session = BrowserSession::connect("ws://localhost:9222").await.unwrap();
    let mut session = session;

    // Navigate to a simple page
    let (url, title) = session.navigate("https://example.com").await.unwrap();
    assert!(url.contains("example.com"));

    // Get snapshot
    let xml = session.snapshot(0).await.unwrap();
    assert!(xml.contains("<heading"));
    assert!(xml.contains("Example Domain"));
    assert!(xml.contains("ref="));

    // Screenshot
    let dir = tempfile::tempdir().unwrap();
    let path = session.screenshot(dir.path()).await.unwrap();
    assert!(path.exists());
    assert!(tokio::fs::metadata(&path).await.unwrap().len() > 0);
}
```

- [ ] **Step 2: Run with Chrome available**

```bash
docker compose up -d chrome
cargo test --features live-tests browser_navigate_and_snapshot
```

- [ ] **Step 3: Commit**

```bash
git add tests/
git commit -m "test: browser tool integration test"
```

---

## Task 13: Final `just ci` pass

- [ ] **Step 1: Run full CI**

```bash
just ci
```

Fix any formatting, clippy, or test issues.

- [ ] **Step 2: Commit fixes if any**

- [ ] **Step 3: Verify the browser tool is hidden when `chrome_cdp_url` is not set**

Confirm `ToolManager::for_chat()` does not include the browser tool when config is
absent.
