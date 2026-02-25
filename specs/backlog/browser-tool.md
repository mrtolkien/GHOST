# Browser Tool — Headless Chrome via CDP

## Status: RESEARCH COMPLETE, NEEDS DECISION

## Motivation

`web_fetch` uses reqwest + readability with a crawl4ai fallback for JS-heavy pages. This
covers ~90% of fetches. A browser tool would handle the remaining cases:

- **Login-gated content** (paid articles, authenticated APIs)
- **Multi-step navigation** (pagination, "Show more" buttons, form submission)
- **Infinite scroll / lazy-loaded content** that crawl4ai's static delay misses
- **Cookie consent walls** requiring interaction

This is NOT a replacement for `web_fetch` — it's a separate tool for when the GHOST
needs to interact with a page rather than just read it.

## Prior Art: OpenClaw Browser

OpenClaw (`openclaw/openclaw`, TypeScript) has a mature browser tool implementation:

- **Architecture**: Local Express HTTP server wrapping CDP + Playwright
- **Accessibility snapshots**: Instead of raw HTML, returns the page's accessibility
  tree with ref IDs for interactive elements. An agent sees:
  ```
  - heading "Welcome" [ref=e3]
  - textbox "Search" [ref=e4]
  - button "Submit" [ref=e5]
  ```
- **Ref-based actions**: Agent says `click ref=e4` — no CSS selectors or XPath needed
- **Single flat tool schema**: One `browser` tool with `action` discriminator field
  (`snapshot`, `act`, `navigate`, `screenshot`, etc.)
- **Token-efficient**: Accessibility tree is 5-20KB vs 500KB-2MB for raw HTML

The accessibility snapshot approach is key — it gives the LLM a semantic view of the
page (what a screen reader sees) rather than a DOM dump.

## Industry Context

Researched: Spider, Firecrawl, Codex CLI, Jina Reader, Trafilatura.

- **Nobody maintains domain-specific extraction rules** except Spider (1000 curated
  scrapers, proprietary). The industry consensus is generic extraction + LLM
  intelligence.
- **Firecrawl CLI** uses `jq` post-processing, not CSS selectors — the agent is the
  intelligence layer.
- **Codex CLI** has zero client-side web fetching — entirely server-side via OpenAI API.
- **OpenClaw** is the only open tool with a real browser-as-tool implementation.

## Recommended Rust Crate: `chromiumoxide`

| Crate             | Async          | CDP Coverage   | Accessibility Tree            | Notes                |
| ----------------- | -------------- | -------------- | ----------------------------- | -------------------- |
| **chromiumoxide** | tokio (native) | Full           | `Accessibility.getFullAXTree` | Best fit             |
| headless_chrome   | sync/blocking  | Full           | Types exist, no API           | Needs spawn_blocking |
| fantoccini        | tokio          | WebDriver only | Not available                 | Wrong protocol       |

`chromiumoxide` matches our stack: tokio, thiserror, tracing. It supports arbitrary CDP
command execution including the full Accessibility domain. Actively maintained (last
release: Jan 2026, maintainer: mattsse).

Note: `chromey` (spider-rs fork of chromiumoxide) is also viable if the original falls
behind on Chrome releases.

## External Dependencies

### Chrome runtime (sidecar container)

The browser tool requires a headless Chrome instance. Two options:

**`chrome-headless-shell`** (recommended):

- Old headless Chrome extracted as standalone binary
- Docker image: `chromedp/headless-shell:stable` (~200 MB compressed)
- RAM: ~150-250 MB idle, ~300-500 MB with a complex page loaded
- Fewer system deps (no X11/Wayland/D-Bus)
- Trade-off: not pixel-identical to real Chrome (bot detection can sometimes tell)

**Full Chrome `--headless`** (if anti-bot matters):

- New headless mode (Chrome 128+): identical to real Chrome
- Docker image: ~400MB-1GB
- More RAM, but better for anti-bot evasion

### Docker setup

```yaml
services:
  chrome:
    image: chromedp/headless-shell:stable
    ports:
      - "9222:9222"
    shm_size: "2gb" # Chrome crashes with default 64MB /dev/shm
    init: true # Prevents zombie helper processes
    security_opt:
      - seccomp=chrome.json # Custom profile for Chrome sandbox
    deploy:
      resources:
        limits:
          memory: 1g
          cpus: "1.0"
```

Critical requirements:

- `shm_size: 2gb` — mandatory, Chrome crashes without it
- `init: true` — mandatory, Chrome spawns helper processes that become zombies
- Security: custom seccomp profile > `SYS_ADMIN` capability > `--no-sandbox`

### Sharing Chrome with crawl4ai

crawl4ai supports `browser_mode: "custom"` with an external CDP URL. Both crawl4ai and
the browser tool can connect to the same Chrome instance:

```
┌─────────────────────────────────────────┐
│  chrome-headless-shell (one instance)   │
│  ws://localhost:9222                    │
└────────┬──────────────────┬─────────────┘
         │ CDP              │ CDP
    ┌────┴────┐      ┌─────┴──────┐
    │ crawl4ai│      │ ghost      │
    │ (custom │      │ browser    │
    │  mode)  │      │ tool       │
    └─────────┘      └────────────┘
```

One Chrome process, two clients. No duplicate instances.

## Proposed Implementation

### Config

```toml
[web]
crawl4ai_url = "http://localhost:11235" # existing
chrome_cdp_url = "ws://localhost:9222" # new — None to disable browser tool
```

### Tool Schema

Single `browser` tool with flat action-based interface:

```json
{
  "name": "browser",
  "description": "Control a headless browser for pages requiring interaction.",
  "input_schema": {
    "type": "object",
    "properties": {
      "action": {
        "type": "string",
        "enum": ["snapshot", "navigate", "click", "type", "scroll", "screenshot"]
      },
      "url": { "type": "string", "description": "URL for navigate action" },
      "ref": { "type": "string", "description": "Element ref ID for actions" },
      "text": { "type": "string", "description": "Text for type action" }
    },
    "required": ["action"]
  }
}
```

### Modules

- `src/web/chrome.rs` — CDP connection via chromiumoxide, accessibility snapshot
  extraction, ref assignment, action dispatch
- `src/tools/browser.rs` — Tool implementation wrapping chrome.rs

### Minimal Viable Feature (~1500-2000 LoC)

1. **Connect** to external Chrome via CDP URL from config
2. **`navigate`** — Open URL in a tab, return accessibility snapshot
3. **`snapshot`** — Get current page's accessibility tree with ref IDs
4. **`click`** / **`type`** / **`scroll`** — Interact by ref ID
5. **`screenshot`** — Capture page as image (useful for debugging)

NOT in scope for MVP:

- Chrome process management (require external sidecar)
- Multiple profiles or extension relay
- Multi-target routing
- File upload, drag-and-drop, PDF generation

### Resource Budget

| Component             | Size / RAM                 | Notes                      |
| --------------------- | -------------------------- | -------------------------- |
| `chromiumoxide` crate | Compile-time only          | ~0 runtime cost            |
| Chrome sidecar        | ~200 MB image, ~500 MB RAM | Shared with crawl4ai       |
| Per tab               | ~100-300 MB additional     | Depends on page complexity |

## Open Questions

- Is this worth the operational complexity of a Chrome sidecar for the 10% of pages that
  need it?
- Should the GHOST be able to decide when to use browser vs web_fetch, or should
  web_fetch auto-escalate?
- Should we wait until there's a concrete use case that crawl4ai can't handle?
