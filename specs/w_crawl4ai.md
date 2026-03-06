Goal: HEAVILY review Crawl4ai usage and web fetch

Status: DONE — implemented in feat/crawl4ai branch

Issues (resolved):

- 40s+ per page → fixed: scan_full_page now false by default,
  domcontentloaded, delay_before_return_html reduced to 0.5s
- Hardcoded strict fetch rules → fixed: agent controls wait_for, css_selector,
  scan_full_page via tool params
- crawl4ai only a fallback → fixed: crawl4ai is now the primary path for all
  HTML. HEAD request routes content types cheaply (no double-fetch).

Crawl4ai param findings (tested empirically):

- wait_until: domcontentloaded, NOT networkidle (times out on most real sites
  with ads/trackers — All3DP, Reddit, Tom's Hardware, PCMag)
- remove_overlay_elements: REMOVED — strips actual content on Wikipedia
  (318K chars → 1 char)
- PruningContentFilter: REMOVED — returns empty on Wikipedia. Use raw_markdown.
- excluded_tags: ["nav", "footer", "header"] — safe noise reduction
- word_count_threshold: 10 — filters tiny text fragments

Implementation:

- HEAD(url) → HTML: crawl4ai, text: reqwest, binary: error
- New tool params: wait_for, css_selector, scan_full_page
- Defaults: domcontentloaded, no scroll, raw_markdown extraction
- Fallback: local extraction (htmd+readability) if crawl4ai fails
- Legacy path preserved for import_page (no crawl4ai overhead)
- Live tests: Wikipedia, Reddit, Tom's Hardware, GitHub, PCMag, CSS selector,
  fallback

Design doc: docs/plans/2026-03-06-crawl4ai-design.md
