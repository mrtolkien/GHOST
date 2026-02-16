# Editable Prompts

## Overview

Make ALL prompts — including the base system prompt — editable as full-text files in the
workspace. The PoC embeds prompts in the binary; this feature externalizes them.

## Goal

```
$WORKSPACE/prompts/
├── system.md           # Base system prompt (currently embedded)
├── compaction.md       # Compaction prompt (currently embedded)
├── heartbeat.md        # Already overridable in PoC
└── reflection.md       # Already overridable in PoC
```

- If a prompt file exists in the workspace, use it instead of the embedded default.
- If no file exists, fall back to the embedded default (same as PoC behavior).
- `ghost init` copies all embedded defaults to `prompts/` so the OPERATOR can see and
  edit them.

## Why

- The OPERATOR (and the GHOST itself during reflection) should be able to tune any
  prompt without recompiling.
- Prompt iteration is the primary tuning surface for the GHOST's behavior. Making it
  require a code change is a bottleneck.
- The GHOST can self-improve by editing its own prompts during reflection.

## Migration from PoC

The PoC already supports workspace overrides for heartbeat and reflection prompts. This
feature generalizes that pattern to all prompts and adds a `prompts/` directory for
organization.
