# Docs Redesign: Ghost in the Shell Aesthetic

**Date**: 2026-03-10 **Status**: Approved

## Summary

Full visual overhaul of the GHOST docs site (Astro Starlight). Dropping the neon
Catppuccin palette in favor of a 90s cyberpunk / Ghost in the Shell inspired aesthetic:
cold terminal, black and white, serif headings, Tokyo Night colors, matrix rain hero.

Tagline: **"Enter the shell"**

## Design Decisions

### Direction: Cold Terminal

- Near-black backgrounds, muted blue-gray text, monochrome with teal accent
- CRT effects (scanlines, matrix rain, glitch) on hero page only
- Content pages are clean and static for reading focus
- No neon glow effects — cold and restrained

### Color Palette (Tokyo Night base)

| Token            | Hex       | Usage                              |
| ---------------- | --------- | ---------------------------------- |
| bg-base          | `#0a0a0a` | Page background                    |
| bg-surface       | `#0d0d0d` | Content area, cards                |
| bg-code          | `#16161e` | Code blocks, inline code           |
| border           | `#1a1b26` | Borders, dividers                  |
| border-highlight | `#292e42` | Hover borders, active states       |
| text-muted       | `#565f89` | Labels, metadata, comments         |
| text-body        | `#a9b1d6` | Body text                          |
| text-heading-2   | `#c0caf5` | H2+ headings                       |
| text-heading-1   | `#e0e0e0` | H1 headings, logo                  |
| accent           | `#1abc9c` | Links, active nav, CTA, borders    |
| terminal-green   | `#9ece6a` | Terminal prompt text, hero tagline |
| warning          | `#e0af68` | Warning callouts                   |

Syntax highlighting uses full Tokyo Night: purple (`#bb9af7`) keywords, blue (`#7aa2f7`)
functions, cyan (`#2ac3de`) types, green (`#9ece6a`) strings, orange (`#ff9e64`)
constants, gray (`#565f89`) comments.

### Typography

| Role     | Font              | Notes                                                                    |
| -------- | ----------------- | ------------------------------------------------------------------------ |
| Headings | DM Serif Display  | Already in use for logo, extend to all headings. No uppercase transform. |
| Body     | IBM Plex Sans     | Weights 400, 500, 600. Replaces system font.                             |
| Code     | Monaspace Krypton | Kept from current design.                                                |
| UI/Nav   | IBM Plex Mono     | Labels, nav items, metadata, terminal elements.                          |

### Hero / Landing Page

- Full-viewport hero with `splash` template
- **Matrix rain**: Canvas-based, teal katakana characters, low opacity (~0.1)
- **Scanline overlay**: Repeating gradient, very subtle green tint
- **CRT vignette**: Radial gradient darkening edges
- **Glitch effect**: Title text with red/blue clip-path offset, triggers every ~3s
- **Typing animation**: "Enter the shell" types in on load
- **Terminal prompt**: `root@ghost:~$` as decorative element above title
- **GH▲ST logo**: DM Serif Display, large, with glitch effect
- **CTAs**: "Get Started" (teal fill) + "GitHub" (ghost border)
- Below fold: Philosophy cards + architecture overview, restyled with new palette

### Site Layout

No structural changes to Starlight layout. Purely CSS reskin:

- **Header**: Near-black, no blur/glass effect. Logo left, search + GitHub right.
- **Sidebar**: Section labels in mono uppercase. Active item gets teal left border +
  subtle teal background tint. Weight 400 (not bold).
- **Content**: Max-width 65ch for body text. Clean, no effects.
- **Search**: Styled to match (dark input, muted placeholder).

### Content Page Styling

- **Headings**: DM Serif Display, no uppercase. H1 `#e0e0e0`, H2+ `#c0caf5`. H2 gets
  bottom border as section divider. No glow.
- **Links**: Teal with subtle underline, intensifies on hover.
- **Code blocks**: `#16161e` background, teal left border, Monaspace Krypton. Tokyo
  Night syntax colors.
- **Inline code**: Same background, cyan text (`#7dcfff`).
- **Callouts**: Dark background + colored left border. Warning: amber. Note: teal.
  Danger: red (#f7768e).
- **Tables**: Minimal borders (`#1a1b26`), no zebra striping.
- **Sidebar active**: Teal left border + `rgba(26,188,156,0.05)` background.
- **Animations**: None on content pages. Hero only. Nav transitions 150ms.

### Light Mode

Deferred to future pass. Dark-only for this iteration. Starlight's toggle infrastructure
remains — just needs a second set of CSS variables later.

## Fonts to Install (npm @fontsource)

- `@fontsource/dm-serif-display` (already installed)
- `@fontsource/ibm-plex-sans` (new — weights 400, 500, 600)
- `@fontsource/ibm-plex-mono` (new — weights 400, 500)
- `@fontsource/monaspace-krypton` (already installed)

Remove: `@fontsource/oxanium` (no longer used)

## Files to Modify

1. `docs/src/styles/starlight-overrides.css` — Full rewrite of CSS variables and styles
2. `docs/src/content/docs/index.mdx` — Hero section with matrix rain canvas + animations
3. `docs/package.json` — Swap font dependencies
4. `docs/public/favicon.svg` — Update colors to match new palette
5. `docs/src/components/SiteTitle.astro` — Update logo colors if needed

## Out of Scope

- Light mode (future pass)
- Content restructuring / sidebar reorg
- New pages or content changes
- Custom Astro components beyond hero
