# Browser Multi-Tab Support

## Status: FUTURE (depends on browser-tool.md MVP)

## Motivation

The browser tool MVP uses a single tab per session — `navigate` replaces the current
page. This covers most use cases but breaks down when the GHOST needs to:

- Compare two pages side by side (e.g., documentation vs implementation)
- Keep a form open while looking up information on another page
- Follow a link without losing the current page's state
- Work across multiple authenticated services simultaneously

## Proposed Actions

Add these actions to the existing `browser` tool:

- **`tabs`** — List open tabs (returns tab ID, URL, title for each)
- **`open`** — Open a new tab (optionally with a URL), returns new tab ID
- **`focus`** — Switch the active tab by tab ID
- **`close`** — Close a tab by tab ID

> [!IMPORTANT] Make sure to review how OpenClaw did it!

## Design Considerations

- **Ref namespacing**: Refs (e.g., `e1`, `e2`) are currently global. With multiple tabs,
  refs must be scoped per tab — either namespaced (`tab1:e3`) or invalidated on tab
  switch (simpler, matches OpenClaw).
- **Active tab**: All existing actions (click, type, scroll, snapshot, screenshot)
  operate on the active tab. `focus` changes which tab is active.
- **Tab limit**: Cap open tabs (e.g., 5-10) to prevent runaway resource usage.
- **Stale tabs**: Tabs may navigate or change state independently (redirects, timers).
  The GHOST should `snapshot` after `focus` to see current state.
- **Session cleanup**: All tabs opened by the GHOST are closed when the session ends.
