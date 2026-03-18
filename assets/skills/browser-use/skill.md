---
name: browser-use
description:
  Read before first use of the browser tool in a session. Covers multi-browser
  management, tab workflow, element refs, operator handoff for auth, and web_fetch
  session sharing.
---

# Browser Use

The `browser` tool lets you control web browsers via Chrome DevTools Protocol (CDP). You
can manage multiple browsers (headless sidecar, operator's Chrome, remote instances) and
multiple tabs per browser.

## Quick Start

1. **If browsers are pre-configured**, just use `navigate` — it auto-connects to the
   first browser and opens a tab.
2. **If no browsers are configured**, use `discover` to find CDP endpoints, or ask the
   OPERATOR to start one.

## Browser Management

### Listing and connecting

- `browsers` — list all known browsers (configured + runtime-added)
- `connect` — connect to a browser by name or CDP URL. Becomes the active browser.
- `disconnect` — disconnect from a browser, close all its tabs.
- `discover` — scan localhost and Tailscale peers for CDP endpoints.

### Active browser

All actions operate on the **active browser**. `connect` sets the active browser. If
only one browser exists, it's auto-activated on first use.

## Tab Management

### Actions

- `open` — open a new tab (optionally with a URL). Becomes the active tab. Returns a
  snapshot.
- `focus` — switch to a tab by ID. Returns a snapshot with fresh element refs.
- `close` — close a tab by ID.
- `tabs` — list open tabs in the active browser.

### Active tab

All interaction actions (navigate, snapshot, click, type, etc.) operate on the **active
tab**. `open` and `focus` change which tab is active.

### Tab limit

Maximum 5 tabs per browser. If you need more, close tabs you're done with first.

### Element refs

Element refs (e1, e2, ...) belong to the tab that produced them. When you `focus` a
different tab, the old refs are invalid — you get fresh refs from the auto-snapshot.

**Rule:** Never use refs from a snapshot of Tab A to interact with Tab B. Always use
refs from the most recent snapshot.

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

Or just click — if the link opens in a new tab (target=\_blank), it's auto-detected and
becomes the active tab.

### Form filling across pages

```
1. Tab 1: source page with data
2. Tab 2: form to fill
3. Read from tab 1, focus tab 2, fill fields
```

## web_fetch and Browser Sessions

`web_fetch` and the browser tool **share the same Chrome session** when both point at
the same browser. This means:

- Log in to a site via the browser tool (navigate, fill credentials, click submit)
- Then use `web_fetch` to read pages on that site — it sees the authenticated cookies
- Much faster than snapshot for reading long content-heavy pages

This works because `web_fetch` routes HTML pages through Crawl4AI, which connects to the
same Chrome instance via CDP and shares its cookie jar.

**When to use which:**

| Scenario                                        | Tool               |
| ----------------------------------------------- | ------------------ |
| Simple page read (no auth)                      | `web_fetch`        |
| Login-gated page (after logging in via browser) | `web_fetch`        |
| Interactive work (forms, clicks, JS-heavy)      | `browser`          |
| Need to see page structure/elements             | `browser` snapshot |

## Operator Handoff

When you hit authentication, decide how to handle it:

- **Username/password auth:** Ask the OPERATOR what they prefer — either give you the
  credentials so you can log in yourself in the headless browser, or log in themselves
  in a live browser you can then control.
- **OAuth, SSO, CAPTCHA, or MFA:** You can't do these yourself. Ask the OPERATOR to
  start a live browser with remote debugging so they can complete the auth flow and you
  take over afterwards.

### Starting the OPERATOR's browser

Ask the OPERATOR to run this on their machine:

```
chromium --remote-debugging-port=9222 --user-data-dir=~/.config/ghost/browser-profile
```

Explain why:

- `--remote-debugging-port=9222` opens CDP so you can control the browser.
- `--user-data-dir=~/.config/ghost/browser-profile` uses a **dedicated profile**,
  separate from their daily browser. This is important: CDP is unauthenticated —
  anything that can reach port 9222 gets full browser control (read cookies, run JS,
  navigate). A dedicated profile keeps the OPERATOR's real passwords and sessions safe.
- The profile is **persistent** — logins, cookies, and site data survive across
  restarts. The OPERATOR only needs to log in to services once.

### Adding the browser

If the OPERATOR's browser is on the same machine, use `ws://localhost:9222`.

If it's on a different machine on the Tailscale network, all ports are open between
peers by default — no extra config needed. Ask the OPERATOR for their Tailscale IP
(`tailscale ip -4`).

Then persist the browser so it's available across reboots:

1. Run `discover` to scan for CDP endpoints (localhost + Tailscale peers).
2. Tell the OPERATOR to run `ghost browsers add operator ws://<ip>:9222` to save it.
3. Tell the OPERATOR to run `ghost config reload` to pick up the new config.

After reload, the OPERATOR's browser is available by name. You can `connect` to
`"operator"` without needing the URL again.

### Workflow

1. **Ask** the OPERATOR to start the browser (give them the command above).
2. **Add** the browser to config and reboot (or `connect` at runtime for one-off use).
3. **Ask** the OPERATOR to log in to any services you need (you can watch via `snapshot`
   and guide them).
4. **Work** in their authenticated session — navigate, fill forms, extract data.
5. **When done**, `disconnect`. The OPERATOR can close the browser or leave it running
   for next time (the profile persists).

## Tool Actions Reference

### Browser management

| Action       | Parameters        | Returns                |
| ------------ | ----------------- | ---------------------- |
| `browsers`   | —                 | List of known browsers |
| `connect`    | `name`, `cdp_url` | Connect + set active   |
| `disconnect` | `name`            | Disconnect browser     |
| `discover`   | —                 | Found CDP endpoints    |

### Tab management

| Action  | Parameters | Returns                 |
| ------- | ---------- | ----------------------- |
| `tabs`  | —          | Tab list                |
| `open`  | `url?`     | Snapshot of new tab     |
| `focus` | `tab`      | Snapshot of focused tab |
| `close` | `tab`      | Confirmation            |

### Interaction (operates on active tab)

| Action       | Key parameters                   |
| ------------ | -------------------------------- |
| `navigate`   | `url`                            |
| `snapshot`   | `offset?`                        |
| `click`      | `ref`                            |
| `type`       | `ref`, `text`                    |
| `scroll`     | `direction`, `ref?`              |
| `screenshot` | —                                |
| `press`      | `key`                            |
| `hover`      | `ref`                            |
| `select`     | `ref`, `value`                   |
| `fill`       | `fields` (array of {ref, value}) |
| `wait`       | `ref?`, `timeout?`               |
| `evaluate`   | `expression`                     |
| `drag`       | `ref`, `target_ref`              |
| `upload`     | `ref`, `path`                    |
| `resize`     | `width`, `height`                |
