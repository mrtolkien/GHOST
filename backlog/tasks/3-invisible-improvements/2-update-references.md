We need a way to update references:

- Git repos = update to latest or tag
- Others = re-trigger crawl and save if different

---

Questions:

- How do we manage versionning?
- Which command do we expose through the CLI?

---

## Design (2026-03-17)

### Decision: `ghost reference update <topic>`

Re-fetches from the original source, diffs against existing references, applies changes.

**CLI:**

- `ghost reference import git --url <url> --topic <name> [--ref <tag>]` — new `--ref`
  flag
- `ghost reference update --topic <name> [--ref <tag>]` — new command

**Config persistence:** `_import.toml` and `import_batch.import_config` (JSON column)
store the full import config (paths, extensions, max_depth, max_pages, git_ref) so
update can replay the import faithfully.

**Diff semantics:**

- New upstream files → create
- Changed files (hash mismatch) → update content + hash, watcher re-embeds
- Unchanged files → skip
- Deleted upstream + NOT cited by notes → delete from DB + disk
- Deleted upstream + cited by notes → move to `references/{topic}/_orphaned/`, update DB
  path, print warning with citing note IDs

**Multi-version:** Use topic hierarchy (`dioxus/docs`, `dioxus/docs-v6`). No special
schema needed.

**No staleness detection** — not needed yet. Operator triggers updates manually or AI
runs the CLI via shell tool.

**Implementation plan:** `backlog/plans/2026-03-17-update-references.md`
