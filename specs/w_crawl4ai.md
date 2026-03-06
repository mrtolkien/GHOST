Goal: HEAVILY review Crawl4ai usage and web fetch

Status: DONE — implemented in feat/crawl4ai branch

Issues (resolved):

- 40s+ per page → fixed: scan_full_page now false by default, networkidle
  replaces fixed 2s delay, delay_before_return_html reduced to 0.5s
- Hardcoded strict fetch rules → fixed: agent controls wait_for, css_selector,
  scan_full_page via tool params
- crawl4ai only a fallback → fixed: crawl4ai is now the primary path for all
  HTML. HEAD request routes content types cheaply (no double-fetch).

Implementation:

- HEAD(url) → HTML: crawl4ai, text: reqwest, binary: error
- New tool params: wait_for, css_selector, scan_full_page
- Defaults: networkidle, remove_overlay_elements, no scroll
- Fallback: local extraction (htmd+readability) if crawl4ai fails
- Legacy path preserved for import_page (no crawl4ai overhead)
- Live tests: Wikipedia speed, all3dp scroll, GitHub JS, css_selector, fallback

Design doc: docs/plans/2026-03-06-crawl4ai-design.md
