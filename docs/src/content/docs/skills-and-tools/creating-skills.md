---
title: Creating Skills
description: When and how to create new skills for your GHOST.
---

import { FileTree } from "@astrojs/starlight/components";

## When to Create a Skill

Create a skill when you find yourself giving the same multi-step instructions repeatedly.
Skills are the cheapest way to extend GHOST — no code changes, no recompilation. Just a
markdown file.

Examples of good skill candidates:

- A code review checklist you always follow
- A specific research methodology for your domain
- A report format you want GHOST to produce consistently

## Skill Format

Each skill lives in `$WORKSPACE/skills/<name>/skill.md` with YAML frontmatter:

```markdown title="skills/my-skill/skill.md"
---
name: my-skill
description: >
  One-line description that helps the GHOST decide when to use this skill. Must be
  specific enough to trigger on relevant queries.
---

# Skill Instructions

Step-by-step workflow for the GHOST to follow.

1. First, do this
2. Then check that
3. Finally, produce this output
```

:::caution
The `description` field is critical — it appears in the system prompt and determines when
the GHOST reads the full skill. A weak description means the skill never triggers.
:::

## Directory Structure

Skills can include supporting files:

<FileTree>

- skills/my-skill/
  - skill.md Main skill definition
  - scripts/ Optional helper scripts
  - references/ Optional reference material
  - assets/ Optional static assets

</FileTree>

## How Discovery Works

Skills are scanned at startup. Their names and descriptions are listed in the system
prompt. When a conversation matches a skill's triggers, the GHOST reads the full skill
file before responding.

## Let GHOST Create Skills

Your GHOST can create skills for itself using the **skill-creator** skill. If it notices a
repeating pattern in your interactions, it can formalize it — or you can ask it to:

> "Create a skill for the way we do weekly planning"
