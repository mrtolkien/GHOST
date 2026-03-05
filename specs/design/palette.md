# Neon Catpuccin Palette

Canonical color reference for GHOST surfaces (docs, Discord embeds, future UIs).

Catpuccin Mocha/Latte base tones (warm purple-blue) with neon-saturated accents. Primary
accent: **Maroon** (Cyberpunk 2077-inspired warm palette). Heading progression: Yellow →
Peach → Maroon (warm descent).

## Neon Catpuccin Mocha (dark)

### Base tones (verbatim Catpuccin Mocha)

| Role      | Name   | HSL                | Hex     |
| --------- | ------ | ------------------ | ------- |
| Deep bg   | Crust  | hsl(240, 23%, 9%)  | #11111b |
| Nav bg    | Mantle | hsl(240, 21%, 12%) | #181825 |
| Main bg   | Base   | hsl(240, 21%, 15%) | #1e1e2e |
| Surface 0 |        | hsl(237, 16%, 23%) | #313244 |
| Surface 1 |        | hsl(234, 13%, 31%) | #45475a |
| Surface 2 |        | hsl(233, 12%, 39%) | #585b70 |

### Text tones (verbatim Catpuccin Mocha)

| Role      | HSL                | Hex     |
| --------- | ------------------ | ------- |
| Text      | hsl(226, 64%, 88%) | #cdd6f4 |
| Subtext 1 | hsl(227, 35%, 80%) | #bac2de |
| Subtext 0 | hsl(228, 24%, 72%) | #a6adc8 |
| Overlay 2 | hsl(228, 17%, 64%) | #9399b2 |
| Overlay 1 | hsl(230, 13%, 55%) | #7f849c |
| Overlay 0 | hsl(231, 11%, 47%) | #6c7086 |

### Neon accents (same hues, saturation cranked)

| Name      | Catpuccin HSL | Neon HSL           | Hex     |
| --------- | ------------- | ------------------ | ------- |
| Yellow    | 41, 86%, 83%  | **41, 100%, 70%**  | #FFD066 |
| Peach     | 23, 92%, 75%  | **23, 100%, 66%**  | #FF8833 |
| Maroon    | 350, 65%, 77% | **350, 100%, 68%** | #FF5C6E |
| Pink      | 316, 72%, 86% | **316, 100%, 72%** | #FF47D1 |
| Red       | 343, 81%, 75% | **343, 100%, 64%** | #FF4778 |
| Flamingo  | 0, 59%, 88%   | **0, 100%, 76%**   | #FF8585 |
| Rosewater | 10, 56%, 91%  | **10, 100%, 82%**  | #FFA399 |
| Sky       | 189, 71%, 73% | **189, 100%, 65%** | #4DDCFF |
| Sapphire  | 199, 76%, 69% | **199, 100%, 62%** | #3DC3FF |
| Blue      | 217, 92%, 76% | **217, 100%, 70%** | #669AFF |
| Lavender  | 232, 97%, 85% | **232, 100%, 78%** | #8F9FFF |
| Mauve     | 267, 84%, 81% | **267, 100%, 72%** | #A94DFF |
| Green     | 115, 54%, 76% | **115, 100%, 62%** | #40FF3D |
| Teal      | 170, 57%, 73% | **170, 100%, 58%** | #29FFD9 |

## Functional Mapping

### Docs (Starlight CSS) — single theme, dark/light mode

**Dark theme (default):**

- `--sl-color-bg`: Crust
- `--sl-color-bg-nav`: Mantle
- `--sl-color-bg-sidebar`: Base
- `--sl-color-accent`: Maroon neon `hsl(350, 100%, 68%)`
- Headings: h1 Yellow, h2 Peach, h3 Maroon, h4 Pink, h5 Red, h6 Flamingo
- Glow layers: Yellow (top-left), Peach (top-right), Teal (bottom, cool counterpoint)

**Light theme:** Catpuccin Latte base tones + warm heading hues (darkened for contrast).

### Discord Embeds

| Constant              | Hex        | Source    |
| --------------------- | ---------- | --------- |
| `GATEWAY_EMBED_COLOR` | `0xFF5C6E` | Maroon    |
| `WARNING_EMBED_COLOR` | `0xFF8833` | Peach     |
| `TOOL_CALL_COLOR`     | `0x6C7086` | Overlay 0 |
| `TODO_COLOR`          | `0xA94DFF` | Mauve     |
