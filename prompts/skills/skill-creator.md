---
name: skill-creator
description:
  Guide for creating effective Agent Skills for GHOST. Use when the OPERATOR wants to
  create a new skill or update an existing skill.
---

# Skill Creator Guide

This guide helps you create effective Agent Skills that extend GHOST's capabilities.

## What is a Skill?

A skill is a self-contained directory with instructions, scripts, and resources that
help you perform specific tasks more accurately and efficiently.

## When to Create a Skill

Create a skill when:

- You have domain-specific knowledge to codify
- You want to provide reusable workflows
- You need to package scripts for common operations
- You want to capture organizational knowledge

## Skill Structure

```
$WORKSPACE/skills/my-skill/
├── skill.md          # Required: Instructions and metadata
├── scripts/          # Optional: Executable code
├── references/       # Optional: Additional docs
└── assets/           # Optional: Static resources
```

## Creating a skill.md

The `skill.md` file has two parts:

### 1. YAML Frontmatter (Required)

```yaml
---
name: my-skill-name
description: Clear description of what this skill does and when to use it.
triggers:
  - trigger phrase one
  - trigger phrase two
---
```

**Naming Rules:**

- 1-64 characters
- Lowercase letters, numbers, hyphens only
- No starting/ending hyphens
- No consecutive hyphens

**Description Tips:**

- Explain WHAT the skill does
- Explain WHEN to use it
- Include keywords for discovery
- Keep it under 1024 characters

### 2. Markdown Body

Write clear, step-by-step instructions. Recommended sections:

```markdown
# Skill Title

## Overview

Brief explanation of the skill's purpose.

## Steps

1. Step one with clear instructions
2. Step two with examples
3. Step three with expected outputs

## Examples

### Example 1: Common Use Case

Input: ... Output: ...

## Common Pitfalls

- Don't do X because...
- Always check Y before...
```

## Best Practices

### Progressive Disclosure

Structure for efficient context usage:

1. **Metadata** (~100 tokens): Loaded at startup for all skills
2. **Instructions** (<5000 tokens): Loaded when skill is activated via `read_file`
3. **Resources** (as needed): Loaded only when required

Keep `skill.md` under 500 lines. Move detailed content to `references/`.

### Writing Instructions

- Use clear, actionable language
- Provide concrete examples
- Include expected inputs and outputs
- Document error cases
- Use code blocks for commands/scripts

### Scripts Directory

Place executable code in `scripts/`:

- Keep scripts self-contained
- Document dependencies
- Handle errors gracefully
- Use descriptive names (e.g., `extract_data.py`, `deploy.sh`)

### References Directory

Place detailed docs in `references/`:

- Technical reference material
- API documentation
- Domain-specific files

Keep individual files focused. Smaller files = less context usage.

## Validation

Before using a skill, verify:

1. Frontmatter is valid YAML with `name` and `description`
2. Name follows conventions (lowercase, hyphens, no consecutive hyphens)
3. Description is clear and actionable
4. Any scripts are tested and working
5. File references use correct relative paths

## Tips for Success

1. **Start Simple**: Create a basic skill first, then expand
2. **Test Iteratively**: Verify the skill works in practice
3. **Document Assumptions**: Explain what context is needed
4. **Be Specific**: Give concrete examples, not vague guidance
5. **Handle Errors**: Document what to do when things go wrong
6. **Keep Updated**: Refresh skills as workflows evolve
