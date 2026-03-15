# Browser Operator Relay — Remote CDP + State Handoff

## Motivation

The browser tool MVP connects to a single headless Chrome sidecar. This works for
autonomous browsing but fails when the GHOST hits login walls, CAPTCHAs, or anything
requiring human eyes/hands. The OPERATOR should be able to:

1. **Watch** the GHOST browse (it's their own browser — zero infrastructure)
2. **Intervene** (log in, solve CAPTCHA, dismiss cookie walls) then hand back
3. **Share a long-lived browser** with persistent cookies, auth, bookmarks

## Core Idea: Remote CDP to OPERATOR's Chrome

Chrome's `--remote-debugging-port` exposes a WebSocket endpoint. Ghost already speaks
CDP via `chromiumoxide`. If the OPERATOR runs Chrome with debugging enabled and exposes
the port over Tailscale, Ghost connects to it identically to a local sidecar — no code
change in the CDP layer.

This collapses watch + interact + autonomous into one: Ghost controls tabs via CDP, the
OPERATOR sees their own browser, and can interact at any time.

## What the MVP Must Get Right for This to Work

The browser tool MVP (browser-tool.md) should:

- **Abstract the CDP target** — `BrowserSession` takes a `ws://` URL, not a hardcoded
  localhost. No assumptions about the Chrome being local or headless.
- **Tab isolation** — Ghost should work in its own tabs, not hijack the OPERATOR's open
  tabs. Open new tabs, close them when done. The OPERATOR's existing tabs are untouched.
- **Graceful disconnection** — if the CDP socket drops (OPERATOR closes Chrome, network
  blip), the tool returns a clear error, not a panic. Reconnect on next tool call.

If these hold, remote CDP is a runtime operation — the GHOST (or OPERATOR via command)
connects to a different CDP target on the fly. No restart, no config edit.

## Features Beyond the MVP

### Dual CDP Targets with State Handoff

Ghost can hold connections to multiple CDP targets simultaneously — a local headless
Chrome and the OPERATOR's remote Chrome. Switching between them is a runtime action
(tool parameter, OPERATOR command, or GHOST decision), not a config change.

Escalation flow when Ghost needs help:

1. Ghost detects it's stuck (login form, CAPTCHA, unexpected block)
2. Exports state from headless via CDP:
   - `Network.getAllCookies()` — all cookies
   - `DOMStorage.getDOMStorageItems()` — localStorage/sessionStorage
   - Current URL
3. Connects to OPERATOR's Chrome, opens new tab, injects state
4. Notifies OPERATOR: "I need you to log in at [URL]"
5. OPERATOR logs in / solves CAPTCHA
6. Ghost detects success (page change, cookie change), continues in OPERATOR's browser —
   or transfers state back to headless

### OPERATOR Visual Access to Headless Chrome

If the OPERATOR doesn't want to run Chrome locally, provide a way to see the headless
sidecar's screen:

- **noVNC**: Run a VNC server alongside headless Chrome, serve via HTTP. The OPERATOR
  opens a URL in their normal browser and sees/controls the headless Chrome.
- **HTTP screenshot stream**: Simpler — periodic screenshots served via a lightweight
  HTTP endpoint. View-only, no interaction.
- **Chrome DevTools frontend**: Chrome's built-in DevTools can connect to a remote
  debugging port. The OPERATOR opens `chrome://inspect` and connects.

### Security

- CDP is **completely unauthenticated**. Anyone who reaches port 9222 has full browser
  control (read passwords, execute JS, exfiltrate data).
- **Tailscale** (or SSH tunnel) is mandatory for remote CDP. Never expose raw port.
- Consider: should Ghost refuse to connect to non-localhost CDP URLs unless the user
  explicitly opts in? (Prevent accidental exposure.)

## Open Questions

- Should Ghost auto-detect "stuck" states (login forms, CAPTCHAs) or should the OPERATOR
  explicitly trigger escalation?
- Should state transfer be bidirectional (headless ↔ operator) or one-way (headless →
  operator, then stay there)?
- Is Tailscale the only supported transport, or should we also support SSH tunnels /
  Cloudflare Tunnel?
