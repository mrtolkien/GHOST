# Browser + crawl4ai Shared Chrome — Risks & Fallbacks

## Status: MONITOR (ships with browser tool MVP, track issues here)

## What We're Doing

The browser tool and crawl4ai share a single Chrome sidecar via CDP. The browser tool
connects directly (chromiumoxide), crawl4ai connects via its `BrowserConfig.cdp_url`
parameter. Cookies are shared — the browser tool can log in, then `web_fetch` via
crawl4ai can access authenticated content.

## Known Risks

### 1. crawl4ai external CDP mode reliability

crawl4ai's `cdp_url` parameter (external browser mode) is less battle-tested than its
default embedded browser. Possible issues:

- **Tab leaks**: crawl4ai may not properly close tabs when using an external browser.
  Leaked tabs accumulate, consuming Chrome memory until it crashes.
- **Connection handling**: crawl4ai may not gracefully handle CDP disconnects/reconnects
  when sharing a browser. Could manifest as hanging requests or cryptic errors.
- **Playwright vs raw CDP conflicts**: crawl4ai uses Playwright internally. Playwright
  has opinions about browser lifecycle that may conflict with an externally-managed
  Chrome. For example, Playwright might try to reset browser state or close contexts
  unexpectedly.

**Fallback**: Extract cookies from the browser tool's Chrome session via CDP
`Network.getAllCookies()`, inject them as request headers into crawl4ai running its own
embedded browser. Loses localStorage/sessionStorage but covers most auth flows (session
cookies). Implementation: ~50 LoC in `crawl4ai.rs`.

### 2. Tab interference

Both the browser tool and crawl4ai operate in the same Chrome instance. While they use
separate tabs, edge cases:

- **`about:blank` routing**: Some CDP clients assume they own the browser's default tab.
  If both try to use it, one gets a stale handle.
- **Browser-level dialogs**: `alert()`, `confirm()`, `beforeunload` dialogs are
  browser-wide and could block crawl4ai if triggered by the browser tool's tab (or vice
  versa).
- **Resource contention**: A very heavy page in the browser tool's tab (e.g., a web app
  eating 500MB) could cause OOM when crawl4ai opens its own tab for extraction.

**Fallback**: Tab namespacing — identify Ghost-owned tabs by a naming convention or
target metadata, so each client ignores the other's tabs. For resource contention,
implement a tab budget (max N concurrent tabs).

### 3. Chrome sidecar stability

Running one Chrome instance under sustained multi-client CDP load:

- **Memory growth**: Chrome leaks memory over time. With two active CDP clients, this
  may hit the docker-compose memory limit (2GB) faster than expected.
- **Zombie processes**: Chrome spawns helper processes. `init: true` in docker-compose
  handles this, but worth monitoring.

**Fallback**: Add a health check endpoint or periodic Chrome restart (e.g., nightly).
Increase memory limit further if 2GB proves insufficient.

### 4. crawl4ai version compatibility

The `cdp_url` parameter may change or break across crawl4ai versions. We're coupling to
an implementation detail of a third-party service.

**Fallback**: Pin crawl4ai Docker image version. If the parameter breaks, fall back to
cookie extraction (risk 1 fallback).

## Monitoring

When the MVP ships, watch for:
- crawl4ai errors specifically in shared-Chrome mode (grep logs for crawl4ai + CDP)
- Chrome sidecar memory usage over time
- Orphaned tabs (tabs that persist after crawl4ai requests complete)
- Any `web_fetch` regressions after switching from embedded to shared Chrome
