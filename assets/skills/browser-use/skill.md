---
name: browser-use
description:
  How to use the browser tool — multi-browser management, tab workflow, operator
  handoff, element refs, and interaction patterns
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

## Operator Handoff

When you hit a login wall, CAPTCHA, or need the OPERATOR's authenticated session:

1. **Ask the OPERATOR** to start a dedicated Chromium with CDP enabled:
   `chromium --remote-debugging-port=9222`
2. **Security:** The OPERATOR should use a separate browser (not their main one).
   Chromium with a fresh profile is ideal. CDP is unauthenticated — anyone who can reach
   the port has full browser control.
3. **Network:** The OPERATOR should expose the port via Tailscale. You can then
   `discover` their browser or `connect` directly with their Tailscale IP.
4. **Authentication:** Ask the OPERATOR to log in to the required service in that
   browser, then you can continue working in their authenticated session.
5. **When done:** `disconnect` from the operator's browser. The OPERATOR can close
   Chromium.

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
