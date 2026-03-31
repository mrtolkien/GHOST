# Weekly Reference Reorganization Agent

A periodic agent that cleans up misclassified or domain-slug reference topics.

1. Lists all topics with `ghost topics list`
2. Identifies domain-slug topics (pattern: `{word}-{tld}` like `btod-com`)
3. For each, reads the reference content and existing notes
4. Proposes moves to semantic topics (e.g., `btod-com` → `hardware/office` or
   `standing-desks`)
5. Executes moves (update DB path + move files on disk) or flags for operator review

This catches anything that slipped through curation misses and import mistakes.

Schedule: weekly, idle trigger. Low priority, background.

Depends on: `5-import-v2/01-import-topic.md` (Fix 1 and Fix 2 should land first so fewer
references end up misclassified going forward).
