---
name: browser-use
description:
  Read before first use of the browser tool in a session. Covers browser selection
  workflow (list → pick → connect), when to use headless vs operator browser, how to
  guide the OPERATOR to expose their browser with `ghost browsers serve`, tab
  management, element refs, and web_fetch session sharing.
---

# Browser Use

The `browser` tool lets you control web browsers via Chrome DevTools Protocol (CDP). You
can manage multiple browsers (headless sidecar, operator's Chrome, remote instances) and
multiple tabs per browser.

## Browser Selection (do this first)

Every browser session starts with picking the right browser.

### 1. List configured browsers

Use the `browsers` action to see what's already registered. This shows configured
browsers and their connection status — it does not scan the network.

### 2. Pick the right browser

| Task                                                                    | Browser    |
| ----------------------------------------------------------------------- | ---------- |
| Autonomous web scraping, reading public pages                           | `headless` |
| Anything the OPERATOR needs to see or interact with (auth, demos, etc.) | `operator` |
| Login-gated sites where you have credentials                            | Either     |
| OAuth, SSO, CAPTCHA, MFA, or "log in for me"                            | `operator` |

If a suitable browser is listed, `connect` to it by name and proceed.

### 3. If no suitable browser exists

- **Need headless**: use `discover` to scan for CDP endpoints on localhost and Tailscale
  peers. If found, `connect` to it.
- **Need operator browser**: guide the OPERATOR through setup (see below). Do not use
  `discover` hoping to find it — if it's not in the list, it's not set up.

### When to ask vs assume

- One browser configured, task doesn't require operator visibility → just use it.
- Multiple browsers, task could go either way → ask which one.
- Task clearly requires one type ("show me" → operator, "scrape this" → headless) → pick
  it without asking.

## Setting Up the OPERATOR's Browser

When the task requires a browser the OPERATOR can see and interact with (auth flows,
demos, supervised browsing), guide them through this setup.

### On the OPERATOR's machine

Tell the OPERATOR to run:

```
ghost browsers serve
```

This command finds a Chromium-based browser, starts it with a dedicated profile
(`~/.config/ghost/browser-profile`), and creates a TCP relay on their Tailscale IP so
the GHOST machine can reach it. It prints the exact `ghost browsers add` command to run
on the GHOST machine.

The dedicated profile is separate from the OPERATOR's daily browser. This matters
because CDP is unauthenticated — anything that can reach the port gets full browser
control. A dedicated profile keeps real passwords and sessions safe.

The profile is persistent — logins, cookies, and site data survive across restarts. The
OPERATOR only needs to log in to services once.

Do not tell the OPERATOR to use `--remote-debugging-address` directly — Chrome's
handling of that flag is unreliable across platforms. `ghost browsers serve` handles
remote access correctly via a relay.

Options: `--port <N>` (default 9222), `--bind <ip>` (auto-detected), `--browser <path>`,
`--profile <dir>`.

### On the GHOST machine

After `ghost browsers serve` prints the connection info, the OPERATOR runs on the GHOST
machine:

```
ghost browsers add operator ws://<tailscale-ip>:9222
ghost config reload
```

The browser is now available by name across reboots.

### Auth workflow

1. Ask the OPERATOR to run `ghost browsers serve` on their machine.
2. Wait for them to add the browser on the GHOST machine and reload config.
3. `connect` to `"operator"` by name.
4. Navigate to the login page. Use `snapshot` so the OPERATOR can see where you are.
5. Hand off — ask the OPERATOR to complete auth in their browser window. Poll with
   `snapshot` until you see the post-login page.
6. Work in their authenticated session — navigate, fill forms, extract data.
7. When done, `disconnect`. The OPERATOR can stop `ghost browsers serve` or leave it
   running (the profile persists).

For username/password auth without OAuth/MFA, ask the OPERATOR what they prefer — give
you the credentials to log in yourself, or do it in their visible browser.

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

Maximum 5 tabs per browser. Close tabs you're done with first.

### Element refs

Element refs (e1, e2, ...) belong to the tab that produced them. When you `focus` a
different tab, the old refs are invalid — you get fresh refs from the auto-snapshot.

Never use refs from a snapshot of Tab A to interact with Tab B. Always use refs from the
most recent snapshot.

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

| Scenario                                        | Tool               |
| ----------------------------------------------- | ------------------ |
| Simple page read (no auth)                      | `web_fetch`        |
| Login-gated page (after logging in via browser) | `web_fetch`        |
| Interactive work (forms, clicks, JS-heavy)      | `browser`          |
| Need to see page structure/elements             | `browser` snapshot |

## Tool Actions Reference

### Browser management

| Action       | Parameters        | Returns                                      |
| ------------ | ----------------- | -------------------------------------------- |
| `browsers`   | —                 | List of configured browsers (no scan)        |
| `connect`    | `name`, `cdp_url` | Connect + set active                         |
| `disconnect` | `name`            | Disconnect browser                           |
| `discover`   | —                 | Scan localhost + Tailscale for CDP endpoints |

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
