# Docs Redesign: Ghost in the Shell — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if
> subagents available) or superpowers:executing-plans to implement this plan. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the GHOST docs site from neon Catppuccin to a Ghost in the Shell /
Cold Terminal aesthetic with Tokyo Night colors.

**Architecture:** Pure CSS reskin of Astro Starlight — no structural changes. One full
rewrite of `starlight-overrides.css`, font dependency swap in `package.json`, updated
hero in `index.mdx` with matrix rain canvas + glitch/typing animations, and a favicon
color update.

**Tech Stack:** Astro Starlight, CSS custom properties, vanilla JS (canvas for matrix
rain), @fontsource packages.

**Design spec:** `specs/backlog/z_docs_colors.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `docs/package.json` | Modify | Swap font deps (remove oxanium, add ibm-plex-sans/mono) |
| `docs/src/styles/starlight-overrides.css` | Rewrite | All CSS variables + overrides |
| `docs/src/content/docs/index.mdx` | Modify | New hero with matrix rain, glitch, typing |
| `docs/src/components/SiteTitle.astro` | Keep | No changes needed (logo stays) |
| `docs/public/favicon.svg` | Modify | Update background color to match new palette |
| `docs/astro.config.mjs` | Keep | No changes needed |

---

## Task 1: Swap Font Dependencies

**Files:**
- Modify: `docs/package.json`

- [ ] **Step 1: Remove oxanium, add IBM Plex Sans and IBM Plex Mono**

```bash
cd docs && npm uninstall @fontsource/oxanium && npm install @fontsource/ibm-plex-sans @fontsource/ibm-plex-mono
```

- [ ] **Step 2: Verify package.json has correct deps**

`package.json` should list:
- `@fontsource/dm-serif-display` (existing)
- `@fontsource/monaspace-krypton` (existing)
- `@fontsource/ibm-plex-sans` (new)
- `@fontsource/ibm-plex-mono` (new)
- No `@fontsource/oxanium`

- [ ] **Step 3: Commit**

```bash
git add docs/package.json docs/package-lock.json
git commit -m "chore(docs): swap fonts — remove oxanium, add IBM Plex Sans/Mono"
```

---

## Task 2: Rewrite CSS — Variables and Base Styles

**Files:**
- Rewrite: `docs/src/styles/starlight-overrides.css`

This is the core task. The entire file gets rewritten. Split into logical sections.

- [ ] **Step 1: Write font imports and `:root` variables**

Replace the file header. New font imports:

```css
@import "@fontsource/dm-serif-display/400.css";
@import "@fontsource/ibm-plex-sans/400.css";
@import "@fontsource/ibm-plex-sans/500.css";
@import "@fontsource/ibm-plex-sans/600.css";
@import "@fontsource/ibm-plex-mono/400.css";
@import "@fontsource/ibm-plex-mono/500.css";
@import "@fontsource/monaspace-krypton/500.css";
```

`:root` variables — Tokyo Night Cold Terminal palette:

```css
:root {
  --sl-sidebar-width: 17.5rem;

  /* Fonts */
  --sl-font: "IBM Plex Sans", system-ui, sans-serif;
  --sl-font-mono: "Monaspace Krypton", "IBM Plex Mono", "JetBrains Mono",
    "Fira Code", monospace;
  --ghost-font-heading: "DM Serif Display", serif;
  --ghost-font-ui: "IBM Plex Mono", monospace;

  /* Tokyo Night — backgrounds */
  --sl-color-black: #0a0a0a;
  --sl-color-bg: #0a0a0a;
  --sl-color-bg-nav: #0a0a0a;
  --sl-color-bg-sidebar: #0a0a0a;
  --sl-color-bg-inline-code: #16161e;

  /* Tokyo Night — text */
  --sl-color-white: #c0caf5;
  --sl-color-gray-1: #c0caf5;
  --sl-color-gray-2: #a9b1d6;
  --sl-color-gray-3: #737aa2;
  --sl-color-gray-4: #565f89;
  --sl-color-gray-5: #292e42;
  --sl-color-gray-6: #16161e;

  /* Accent — teal */
  --sl-color-accent-low: rgba(26, 188, 156, 0.1);
  --sl-color-accent: #1abc9c;
  --sl-color-accent-high: #73daca;

  --sl-color-text: var(--sl-color-gray-2);
  --sl-color-text-accent: var(--sl-color-accent);
  --sl-color-text-invert: #0a0a0a;

  /* Borders */
  --sl-color-hairline-light: #292e42;
  --sl-color-hairline: #1a1b26;
  --sl-color-hairline-shade: #0a0a0a;

  /* Heading colors — monochrome, no neon */
  --ghost-h1: #e0e0e0;
  --ghost-h2: #c0caf5;
  --ghost-h3: #c0caf5;
  --ghost-h4: #a9b1d6;
  --ghost-h5: #a9b1d6;
  --ghost-h6: #a9b1d6;

  /* Accent colors for callouts/syntax */
  --ghost-accent: #1abc9c;
  --ghost-green: #9ece6a;
  --ghost-warning: #e0af68;
  --ghost-error: #f7768e;
  --ghost-info: #7dcfff;
}
```

- [ ] **Step 2: Write body, header, and sidebar styles**

```css
body {
  background-color: var(--sl-color-bg);
}

/* Header — clean, dark, no effects */
.header {
  background: rgba(10, 10, 10, 0.95) !important;
  background-image: none !important;
  border: 0 !important;
  box-shadow: none !important;
  backdrop-filter: none !important;
  overflow: visible !important;
  border-bottom: 1px solid #1a1b26 !important;
}

.header :is(.site-title, site-search button, site-search dialog,
    starlight-theme-select label, .social-icons::after) {
  border: 0 !important;
  box-shadow: none !important;
}

/* Sidebar */
.sidebar-pane,
.right-sidebar-panel {
  box-shadow: none;
}

.sidebar-content :is(a, summary, .large, .group-label span) {
  font-weight: 400;
  letter-spacing: 0.02em;
}

.sidebar-content .group-label span {
  font-family: var(--ghost-font-ui);
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: #565f89;
}

.sidebar-content a[aria-current="page"] {
  color: var(--sl-color-white);
  border-inline-start: 2px solid var(--ghost-accent);
  background: rgba(26, 188, 156, 0.05);
  box-shadow: none;
}

.right-sidebar-panel a[aria-current="true"] {
  color: var(--sl-color-white);
  box-shadow: none;
}
```

- [ ] **Step 3: Write heading and content styles**

```css
/* Headings — DM Serif Display, no uppercase, no glow */
.sl-markdown-content :is(h1, h2, h3, h4, h5, h6),
.sl-markdown-content .sl-heading-wrapper > :first-child {
  font-family: var(--ghost-font-heading);
  font-weight: 400;
  letter-spacing: 0.02em;
  text-transform: none;
  text-shadow: none;
}

.sl-markdown-content h1,
.sl-markdown-content .sl-heading-wrapper.level-h1 > :first-child {
  color: var(--ghost-h1);
}

.sl-markdown-content h2,
.sl-markdown-content .sl-heading-wrapper.level-h2 > :first-child {
  color: var(--ghost-h2);
  padding-bottom: 0.4rem;
  border-bottom: 1px solid #1a1b26;
}

.sl-markdown-content h3,
.sl-markdown-content .sl-heading-wrapper.level-h3 > :first-child {
  color: var(--ghost-h3);
}

.sl-markdown-content h4,
.sl-markdown-content .sl-heading-wrapper.level-h4 > :first-child {
  color: var(--ghost-h4);
}

.sl-markdown-content h5 { color: var(--ghost-h5); }
.sl-markdown-content h6 { color: var(--ghost-h6); }

/* Page title — same treatment, no uppercase */
h1#starlight__overview,
h1[data-page-title],
.main-pane > h1,
.content-panel .sl-container > h1#_top {
  font-family: var(--ghost-font-heading);
  font-weight: 400;
  text-transform: none;
  letter-spacing: 0.02em;
  color: var(--ghost-h1);
  text-shadow: none;
}

/* Links — teal, subtle underline */
.sl-markdown-content a {
  color: var(--ghost-accent);
  text-decoration: none;
  border-bottom: 1px solid rgba(26, 188, 156, 0.3);
  transition: border-color 0.15s;
}

.sl-markdown-content a:hover {
  border-bottom-color: var(--ghost-accent);
  text-shadow: none;
}

.sidebar-content a:hover,
.right-sidebar-panel a:hover {
  color: var(--sl-color-white);
  text-shadow: none;
}

/* Inline code */
.sl-markdown-content code {
  font-family: var(--__sl-font-mono);
  color: var(--ghost-info);
  background: var(--sl-color-bg-inline-code);
}

/* Code blocks — teal left border */
.sl-markdown-content pre,
.expressive-code pre {
  border-left: 3px solid var(--ghost-accent);
}

.sl-markdown-content code,
.sl-markdown-content pre code,
.expressive-code code {
  font-family: var(--__sl-font-mono);
}

/* CTA buttons */
.sl-link-button.primary {
  box-shadow: none;
  background: var(--ghost-accent);
  border-color: var(--ghost-accent);
}

.sl-link-button.primary:hover {
  background: var(--sl-color-accent-high);
  border-color: var(--sl-color-accent-high);
}

/* Pagination */
.pagination-links a {
  border-color: #1a1b26;
  box-shadow: none;
}

.pagination-links a:hover {
  border-color: #292e42;
}
```

- [ ] **Step 4: Write right sidebar (TOC) styles**

```css
/* Right sidebar TOC — no colored heading tiers, just muted */
.right-sidebar-panel h2 {
  font-family: var(--ghost-font-ui);
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.12em;
  color: #565f89;
}

.right-sidebar-panel starlight-toc a {
  color: #737aa2;
  transition: color 0.15s;
}

.right-sidebar-panel starlight-toc a:hover {
  color: var(--sl-color-white);
}

mobile-starlight-toc a {
  color: #737aa2;
}
```

- [ ] **Step 5: Write GHOST logo styles (kept from current, cleaned up)**

```css
/* ── GHOST logo ── */
.ghost-logo {
  display: inline-flex;
  align-items: baseline;
  line-height: 1;
  white-space: nowrap;
  color: #e0e0e0;
}

.ghost-logo-letters {
  font-family: var(--ghost-font-heading);
  font-weight: 400;
  color: currentColor;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.ghost-logo-o-slot {
  position: relative;
  display: inline-flex;
  align-items: baseline;
  justify-content: center;
}

.ghost-logo-triangle {
  height: 0.71em;
  width: 0.82em;
  vertical-align: baseline;
}

.ghost-logo-o-inside {
  position: absolute;
  font-family: var(--ghost-font-heading);
  font-weight: 400;
  color: currentColor;
  text-transform: uppercase;
  left: 50%;
  transform: translateX(-50%);
  bottom: -0.08em;
  font-size: 0.58em;
}

.ghost-logo-link {
  text-decoration: none;
  font-size: 1.5rem;
  text-transform: none;
  letter-spacing: normal;
}
```

- [ ] **Step 6: Write hero-specific styles (animations for landing page)**

```css
/* ── Hero page ── */
.hero .ghost-logo {
  font-size: clamp(3rem, 8vw, 5rem);
}

/* Hero glitch effect on title */
@keyframes ghost-glitch-top {
  0%, 92% { transform: translate(0); }
  93% { transform: translate(-3px, -1px); }
  94% { transform: translate(2px, 1px); }
  95% { transform: translate(0); }
}

@keyframes ghost-glitch-bottom {
  0%, 94% { transform: translate(0); }
  95% { transform: translate(3px, 1px); }
  96% { transform: translate(-2px, -1px); }
  97% { transform: translate(0); }
}

@keyframes ghost-typing {
  from { width: 0; }
  to { width: 15ch; }
}

@keyframes ghost-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0; }
}

@keyframes ghost-fade-in {
  to { opacity: 1; }
}
```

- [ ] **Step 7: Remove all light mode styles and old animations**

Delete the entire `:root[data-theme="light"]` block and all old `@keyframes`
(`ghost-glow`, `ghost-glitch`, `ghost-glow-light`, `ghost-glitch-light`). Remove old
background gradient layers, neon text-shadow rules, and maroon-colored box-shadows.

- [ ] **Step 8: Add responsive media query**

```css
@media (min-width: 90rem) {
  :root {
    --sl-content-width: 55rem;
  }

  .concepts-grid-3up .card-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .concepts-grid-3up .card {
    padding: clamp(0.5rem, calc(0.0625rem + 1.5vw), 1.25rem);
  }
}
```

- [ ] **Step 9: Verify the full file is coherent, no duplicate selectors**

Read through the complete file. Ensure no leftover Catppuccin references, no neon glow
rules, no light mode variables.

- [ ] **Step 10: Commit**

```bash
git add docs/src/styles/starlight-overrides.css
git commit -m "feat(docs): rewrite CSS to Cold Terminal / Tokyo Night aesthetic"
```

---

## Task 3: Update Hero / Landing Page

**Files:**
- Modify: `docs/src/content/docs/index.mdx`

The hero needs to switch from the static Starlight splash to a custom hero with matrix
rain canvas, glitch title, typing tagline, and CRT effects. The below-fold content
(Philosophy cards, Architecture table) stays.

- [ ] **Step 1: Replace the frontmatter hero section**

Change the tagline and keep the logo + CTAs. The new tagline is "Enter the shell."
Since Starlight's `splash` template renders the hero from frontmatter, we keep using it
but update the tagline:

```yaml
---
title: GHOST
description:
  Personal AI agent platform with persistent memory, background jobs, and
  multi-interface communication.
template: splash
hero:
  title: |
    <span class="ghost-logo ghost-hero-title" data-text="GHOST">
      <span class="ghost-logo-letters">GH</span>
      <span class="ghost-logo-o-slot">
        <svg viewBox="0 0 116 100" class="ghost-logo-triangle" aria-hidden="true">
          <polygon points="58,0 116,100 0,100" fill="none" stroke="currentColor" stroke-width="4" stroke-linejoin="round"/>
        </svg>
        <span class="ghost-logo-o-inside">O</span>
      </span>
      <span class="ghost-logo-letters">ST</span>
    </span>
  tagline: Enter the shell.
  actions:
    - text: Get Started
      link: /getting-started/installation/
      icon: right-arrow
      variant: primary
    - text: GitHub
      link: https://github.com/mrtolkien/ghost
      icon: external
      variant: minimal
---
```

- [ ] **Step 2: Add matrix rain canvas and CRT effects via inline script**

After the frontmatter and import, before the Philosophy section, add a `<script>`
block and supporting markup for the hero effects. Since Starlight renders the hero from
frontmatter, we'll inject the canvas and overlays via client-side JS that targets the
`.hero` container:

```html
<script>
  // Matrix rain + CRT effects — runs on page load for splash page only
  document.addEventListener('DOMContentLoaded', () => {
    const hero = document.querySelector('.hero');
    if (!hero) return;

    hero.style.position = 'relative';
    hero.style.overflow = 'hidden';

    // Scanlines overlay
    const scanlines = document.createElement('div');
    scanlines.style.cssText = `
      position:absolute;inset:0;z-index:1;pointer-events:none;
      background:repeating-linear-gradient(0deg,transparent,transparent 2px,
        rgba(0,255,65,0.015) 2px,rgba(0,255,65,0.015) 4px);
    `;
    hero.prepend(scanlines);

    // CRT vignette
    const vignette = document.createElement('div');
    vignette.style.cssText = `
      position:absolute;inset:0;z-index:2;pointer-events:none;
      background:radial-gradient(ellipse at center,transparent 50%,rgba(0,0,0,0.6) 100%);
    `;
    hero.prepend(vignette);

    // Matrix rain canvas
    const canvas = document.createElement('canvas');
    canvas.style.cssText = 'position:absolute;inset:0;z-index:0;opacity:0.1;';
    hero.prepend(canvas);

    const ctx = canvas.getContext('2d');
    function resize() {
      canvas.width = hero.offsetWidth;
      canvas.height = hero.offsetHeight;
    }
    resize();
    window.addEventListener('resize', resize);

    const chars = 'ゴーストシェルアイデンティティ記憶知識01';
    const fontSize = 14;
    let columns = Math.floor(canvas.width / fontSize);
    let drops = Array(columns).fill(0).map(() => Math.random() * -100);

    function draw() {
      ctx.fillStyle = 'rgba(10, 10, 10, 0.08)';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.fillStyle = '#1abc9c';
      ctx.font = fontSize + 'px monospace';

      for (let i = 0; i < drops.length; i++) {
        const char = chars[Math.floor(Math.random() * chars.length)];
        const x = i * fontSize;
        const y = drops[i] * fontSize;
        if (y > 0) {
          ctx.globalAlpha = Math.random() * 0.5 + 0.1;
          ctx.fillText(char, x, y);
        }
        if (y > canvas.height && Math.random() > 0.98) drops[i] = 0;
        drops[i] += 0.4 + Math.random() * 0.3;
      }
      ctx.globalAlpha = 1;
      requestAnimationFrame(draw);
    }
    draw();

    // Ensure hero content is above overlays
    const heroContent = hero.querySelector(':scope > div');
    if (heroContent) heroContent.style.position = 'relative';
    if (heroContent) heroContent.style.zIndex = '10';
  });
</script>
```

- [ ] **Step 3: Verify the Philosophy and Architecture sections are unchanged**

The `<CardGrid>`, `<Card>`, and `<table>` blocks below the hero should remain exactly
as-is. Only the frontmatter and the added script block change.

- [ ] **Step 4: Commit**

```bash
git add docs/src/content/docs/index.mdx
git commit -m "feat(docs): add matrix rain hero with glitch + CRT effects"
```

---

## Task 4: Update Favicon

**Files:**
- Modify: `docs/public/favicon.svg`

- [ ] **Step 1: Update the background color from `#1a1a2e` to `#0a0a0a`**

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128">
  <rect width="128" height="128" rx="16" fill="#0a0a0a"/>
  <polygon
    points="64,8 122,108 6,108"
    fill="none"
    stroke="white"
    stroke-width="10"
    stroke-linejoin="round"
  />
  <text
    x="64"
    y="100"
    text-anchor="middle"
    font-family="system-ui, serif"
    font-weight="700"
    font-size="70"
    fill="white"
  >O</text>
</svg>
```

- [ ] **Step 2: Commit**

```bash
git add docs/public/favicon.svg
git commit -m "chore(docs): update favicon background to match new palette"
```

---

## Task 5: Build Verification

- [ ] **Step 1: Install dependencies and build**

```bash
cd docs && npm install && npm run build
```

Expected: Build succeeds with no errors. Warnings about unused CSS are OK.

- [ ] **Step 2: Preview and visual check**

```bash
cd docs && npm run preview
```

Open `http://localhost:4321` and verify:
- Hero: matrix rain, glitch on logo, "Enter the shell." tagline
- Dark background throughout
- DM Serif Display headings, IBM Plex Sans body text
- Teal accent on links, active sidebar items, code block borders
- No neon glow effects anywhere
- No Catppuccin colors remaining
- Sidebar section labels in mono uppercase
- Code blocks have teal left border

- [ ] **Step 3: Check a content page**

Navigate to any content page (e.g., Knowledge Base). Verify:
- Headings are DM Serif Display, normal case (not uppercase)
- H2 has bottom border divider
- Body text is readable `#a9b1d6` on `#0d0d0d`
- No CRT effects on content pages
- Inline code is cyan on dark

- [ ] **Step 4: Commit any fixes if needed, then final commit**

```bash
git add -A docs/
git commit -m "feat(docs): complete Ghost in the Shell visual redesign"
```
