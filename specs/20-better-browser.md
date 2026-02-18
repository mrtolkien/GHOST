## Hybrid Web Retrieval + Full Browser Capability (Node Playwright Sidecar)

### Issue

Accessing many simple articles, for example:

- https://all3dp.com/1/snapmaker-u1-reviewed-make-haste-not-waste/
- https://www.rtings.com/monitor/reviews/best/by-usage/business-office

Completely fails with our web fetch tool.

### Summary

Build a tiered retrieval system for ghost web fetch that maximizes success on
anti-bot/JS-heavy pages, while keeping cost controlled for <$100/mo. Implementation
defaults to automated tiers first; when all tiers fail in v1, return a structured
challenge package (no live manual UI yet). Architecturally, use a Node Playwright
sidecar for browser-grade retrieval and reserve a full browser control toolset as phase
2 on the same sidecar.

### Scope and Success Criteria

- Replace single-shot HTTP fetch behavior with deterministic multi-tier fallback.
- Achieve materially higher success on Cloudflare/challenge domains than current
  reqwest-only flow.
- Work on headless homelab/server environments.
- Preserve fast/cheap path for normal pages.
- Provide clear failure artifacts for operator follow-up when blocked.

Success is:

1. ghost web fetch <url> returns extractable content for most previously failing
   JS/challenge pages.
2. Hard failures return actionable challenge artifacts, not generic 403/429.
3. Tier usage/cost/success is observable per host.

### Final Decisions Locked

- Strategy: Hybrid fetch + browser
- Browser architecture: Node Playwright sidecar
- Policy: Max retrieval
- Budget target: < $100/mo
- CAPTCHA/challenge policy: Operator-assist in principle, but v1 has no live UI
- v1 unsolved behavior: Return challenge package
- Manual live UI (noVNC/etc): deferred to phase 3

———

## Architecture

### 1) Tiered Fetch State Machine

For each URL, execute tiers in order until success:

1. http_direct
2. http_reader_transform (low-cost reader endpoint fallback)
3. firecrawl_scrape (managed extraction)
4. browser_playwright_sidecar (headless browser extraction)
5. wayback_lookup (historical snapshot fallback)

archive.is is not a dedicated tier (too challenge-prone for bots). If user gives an
archive URL directly, it still goes through tiers.

### 2) Escalation Triggers

Escalate when any of:

- HTTP 403/429/503
- challenge fingerprints in title/body (Cloudflare/CAPTCHA/interstitial)
- extraction quality below threshold (very low text density, challenge-like output)
- JS-required blank pages

### 3) Host Memory

Store per-host outcomes and preferred tier in workspace metadata:

- hostname
- last successful tier
- recent failure signatures
- cooldown/backoff windows

Use this to skip known-failing cheap tiers on repeated hosts.

### 4) Sidecar Contract (Rust ↔ Node)

Rust calls local/remote sidecar over HTTP with strict timeout/retry policy.

Endpoints:

- POST /fetch body: { url, timeoutMs, waitUntil, maxChars, userAgentProfile,
  cookiesProfile? }
- POST /extract body: { html, url, mode: "readability|markdown|raw" } (optional if
  extraction stays in Rust)
- GET /health

/fetch response:

- success: { ok: true, finalUrl, status, title?, html?, text?, screenshotPath?, metrics
  }
- blocked: { ok: false, blocked: true, blockerType, status?, htmlSnippet?,
  screenshotPath?, metrics }
- error: { ok: false, blocked: false, errorCode, message, metrics }

### 5) Challenge Package (v1 terminal output)

When all tiers fail, return and cache:

- blocker classification
- per-tier attempt log
- final HTML snippet
- screenshot (if browser tier ran)
- suggested next action

Saved under workspace (e.g. .web-cache/challenges/...).

### CLI

Extend ghost web fetch:

- --strategy auto|http-only|browser-only
- --max-tier <n>
- --no-provider (skip paid provider tiers)
- --emit-challenge-package (default true)

Add:

- ghost web fetch-debug <url> (prints tier-by-tier diagnostics)
- Phase 2: ghost browser ... command family
  (status/start/stop/snapshot/screenshot/navigate/act)

### Config (config.toml)

Add:

- [web.fetch]
  - strategy = "auto"
  - max_tier = 5
  - timeout_ms = 30000
  - challenge_package = true
- [web.fetch.providers.firecrawl]
  - enabled = true
  - api_key_env = "FIRECRAWL_API_KEY"
- [web.fetch.providers.browser]
  - enabled = true
  - base_url = "http://127.0.0.1:PORT"
  - timeout_ms = 45000
- [web.fetch.providers.wayback]
  - enabled = true
- [web.fetch.cost_guardrails]
  - per_request_soft_limit_usd
  - monthly_soft_limit_usd

### Rust Types

- FetchTier, FetchAttempt, BlockerType, ChallengePackage, FetchDiagnostics
- WebError additions:
  - Blocked { blocker, tier, status }
  - Exhausted { attempts, challenge_package_path }
  - provider-specific typed errors

———

## Observability (Required)

Instrument:

- web.fetch.run span: URL, host, strategy, final outcome
- web.fetch.attempt span per tier: tier, elapsed, status, blocker, chars extracted
- structured logs on escalation, provider call, and terminal failure
- emit challenge package path on failure

———

## Testing and Acceptance

### Unit

- challenge fingerprint detection
- escalation decisions
- host-memory tier preference logic
- challenge package serialization

### Integration (mocked providers)

- 200 direct success stops at tier 1
- 403 challenge escalates to browser tier and succeeds
- provider failure falls through to next tier
- total failure produces challenge package artifact + stable error type

### Live tests (--features live-tests, human-run only)

- known simple page (tier 1 success)
- known JS-heavy page (browser/provider success)
- known challenge page (blocked path with challenge package)
- archive/wayback snapshot success case

Acceptance:

- no generic non-success status 403/429 as final user-facing output in auto mode
- meaningful diagnostics + cached artifacts on terminal failure

———

## Rollout Plan

### Phase 1 (core)

- Tiered fetch in existing web fetch
- Firecrawl + sidecar + wayback integrations
- challenge package output + caching
- observability + tests

### Phase 2 (full browser capability)

- add ghost browser CLI/tool surface on same sidecar
- snapshot/ref-based interactions and screenshots
- reuse same security model for untrusted browser content

### Phase 3 (manual solve UI)

- optional self-hosted noVNC session handoff over authenticated HTTP
- resumable job continuation after manual solve

———

## Assumptions and Defaults

- Node sidecar is acceptable operationally on homelab.
- Managed provider usage is allowed within configured budget.
- v1 will not include interactive manual challenge solving UI.
- When unresolved, v1 returns challenge package (not silent fail, not infinite retries).
- Existing text extraction (readability/markdown) remains as post-fetch extraction path.
