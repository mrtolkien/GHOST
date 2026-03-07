---
name: image-generation
description:
  Generate or edit images with Nano Banana 2 (Gemini 3.1 Flash Image). Use for any image
  create/modify requests. Supports text-to-image + image-to-image; 1K/2K/4K resolutions.
---

# Image Generation (Nano Banana 2)

Generate new images or edit existing ones using Google's Gemini 3.1 Flash Image API.

## Usage

Run the bundled script via `run_shell_command`. Images are saved to `$WORKSPACE/tmp/`.

**Generate new image:**

```
run_shell_command command="uv run $WORKSPACE/skills/image-generation/scripts/generate_image.py --prompt 'your image description' --filename '$WORKSPACE/tmp/yyyy-mm-dd-hh-mm-ss-name.png'"
```

**Edit existing image:**

```
run_shell_command command="uv run $WORKSPACE/skills/image-generation/scripts/generate_image.py --prompt 'editing instructions' --filename '$WORKSPACE/tmp/yyyy-mm-dd-hh-mm-ss-name.png' --input-image 'path/to/input.png'"
```

**Resolution option:** Append `--resolution 1K|2K|4K` (default: 1K).

## Workflow (draft -> iterate -> final)

Fast iteration without burning time on 4K until the prompt is correct.

1. **Draft (1K)**: quick feedback loop — generate at default resolution
2. **Iterate**: adjust prompt in small diffs; use a new filename per run. If editing,
   keep the same `--input-image` for every iteration until happy.
3. **Final (4K)**: only when prompt is locked — add `--resolution 4K`

## Resolution Mapping

- No mention of resolution -> `1K`
- "low resolution", "1080", "1080p", "1K" -> `1K`
- "2K", "2048", "normal", "medium resolution" -> `2K`
- "high resolution", "high-res", "hi-res", "4K", "ultra" -> `4K`

## API Key

The script reads `GEMINI_API_KEY` from the environment (set in `.env`). If missing, it
exits with an error.

## Filename Convention

`yyyy-mm-dd-hh-mm-ss-descriptive-name.png`

- Timestamp in 24-hour format
- Descriptive lowercase text with hyphens (1-5 words)
- Derive from the user's prompt or conversation context
- If unclear, use a random identifier (e.g. `x9k2`)

## Prompt Handling

**For generation:** Pass the OPERATOR's image description as-is to `--prompt`. Only
rework if clearly insufficient.

**For editing:** Pass editing instructions in `--prompt` (e.g. "add a rainbow in the
sky", "make it look like a watercolor painting").

Preserve the OPERATOR's creative intent in both cases.

### Prompt Templates (for vague requests)

- **Generation**: "Create an image of: <subject>. Style: <style>. Composition:
  <camera/shot>. Lighting: <lighting>. Background: <background>. Color palette:
  <palette>. Avoid: <list>."
- **Editing** (preserve everything else): "Change ONLY: <single change>. Keep identical:
  subject, composition/crop, pose, lighting, color palette, background, text, and
  overall style. Do not add new objects."

## Common Failures

- `Error: No API key provided.` -> set `GEMINI_API_KEY` in `.env`
- `Error loading input image:` -> wrong path or unreadable file
- 403 / quota errors -> wrong key, no access, or quota exceeded

## Output

- Saves PNG to the specified path (always under `$WORKSPACE/tmp/`)
- Script outputs the full path to the generated image
- **Do not read the image back** — just inform the OPERATOR of the saved path
