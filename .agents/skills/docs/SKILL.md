---
name: docs
description: >-
  Documentation conventions for Ghost's user-facing docs site (Astro Starlight). MUST
  READ before creating or updating any page under docs/, modifying CLI help text, or
  changing user-facing behavior that needs documentation. Covers which doc file to
  update for each change type, build verification, formatting, and terminology rules.
---

# Documentation Conventions

User-facing docs live in `docs/` (Astro Starlight).

## Which File to Update

| Change type         | Doc file                                                 |
| ------------------- | -------------------------------------------------------- |
| CLI changes         | `docs/src/content/docs/reference/cli.md`                 |
| Config changes      | `docs/src/content/docs/getting-started/configuration.md` |
| Tool changes        | `docs/src/content/docs/features/tools-*.md`              |
| New features        | relevant page under `docs/src/content/docs/features/`    |
| Workspace/bootstrap | `docs/src/content/docs/getting-started/workspace.mdx`    |
| Provider changes    | `docs/src/content/docs/ghost/providers.md`               |

## Build Verification

Always verify docs build after changes:

```sh
cd docs && npm run build
```

## Formatting

- Docs content (`docs/src/content/**`) is excluded from oxfmt because Starlight's `:::`
  admonition syntax is not supported by Prettier-compatible formatters.
- Line width: 88 characters for all other markdown/MDX/JSON/TOML (via oxfmt).

## Terminology

Always write `GHOST` and `OPERATOR` in all caps in prose. Keep real code/file
identifiers (crate names, paths, variable names) unchanged.
