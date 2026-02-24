# Skills

Skills are workflow files that teach your GHOST how to handle specific tasks. They
follow the [agentskills.io](https://agentskills.io) format.

## Skill Format

Each skill lives in `$WORKSPACE/skills/<name>/skill.md` with YAML frontmatter:

```markdown
---
name: my-skill
description: >
  One-line description that helps the GHOST decide when to use this skill.
  Must be specific enough to trigger on relevant queries.
---

# Skill Instructions

Step-by-step workflow for the GHOST to follow.

1. First, do this
2. Then check that
3. Finally, produce this output
```

The `description` field is critical — it appears in the system prompt and determines
when the GHOST reads the full skill. A weak description means the skill never triggers.

## Default Skills

| Skill                   | Purpose                                                                      |
| ----------------------- | ---------------------------------------------------------------------------- |
| **research**            | Multi-source web research for recommendations, comparisons, buying decisions |
| **knowledge-navigator** | Querying the knowledge base effectively (search, graph, tags)                |
| **note-writer**         | Creating structured knowledge notes with archetypes and wiki links           |
| **cron-job-author**     | Creating and improving cron-scheduled job files                              |
| **skill-creator**       | Meta-skill: creating new skills                                              |

## How Discovery Works

Skills are scanned at startup. Their names and descriptions are listed in the system
prompt. When a conversation matches a skill's triggers, the GHOST reads the full skill
file before responding.

## Writing Your Own

1. Create `~/GHOST/skills/my-skill/skill.md`
2. Write a strong `description` — this is what triggers the GHOST to use it
3. Include clear, step-by-step instructions in the body
4. Optionally add supporting files in `scripts/`, `references/`, or `assets/`
   subdirectories

Your GHOST can also create skills for itself using the **skill-creator** skill.

## Skill Directory Structure

```
skills/my-skill/
├── skill.md           # Main skill definition
├── scripts/           # Optional helper scripts
├── references/        # Optional reference material
└── assets/            # Optional static assets
```
