# FlowClone User Guide Site — Design Spec

**Date:** 2026-06-30
**Status:** Approved (brainstorming)

## Goal

Publish the FlowClone user guide as a single-page, app-branded website on GitHub
Pages. v1 is **English only** (Thai planned for a later version).

## Decisions (locked)

- **Structure:** single page with a sticky sidebar table of contents (scroll-spy).
- **Build approach:** hand-built static HTML + CSS, no build step, no JS framework.
- **Content source:** ported from `docs/USER_GUIDE.md` (8 sections).
- **Hosting:** GitHub Pages via a GitHub Actions workflow (`actions/deploy-pages`)
  that publishes the `site/` folder. Not "deploy from /docs" — `docs/` already
  holds markdown docs.
- **Language:** EN only for v1.

## Look & feel

Match the desktop app's light theme (anti-template, intentional design):

| Token | Value |
| --- | --- |
| Primary | `#3b82f6` |
| Background | `#f7f8fa` |
| Surface | `#ffffff` |
| Text | `#111827` |
| Muted | `#6b7280` |
| Border | `#e5e7eb` |
| Warning | `#f59e0b` |
| Danger | `#ef4444` |

- **Hero:** blue gradient (`#1e3a8a → #3b82f6`), app logo, tagline
  "Move everything. Lose nothing.", primary **Download DMG** button (latest
  release) + secondary **View on GitHub**.
- **Layout:** sticky left sidebar TOC on desktop; collapses to a top nav on
  mobile (≤ 860px). Content in clean white cards with soft shadows and clear
  type hierarchy.
- **Callouts:** distinct warning/danger boxes for the destructive-restore
  warnings (target erase).
- CLI commands in monospace `<code>`. Designed hover/focus states. Honors
  `prefers-reduced-motion`.

## Content sections (single page, anchored)

1. Install & first launch (macOS + Windows)
2. The home screen — includes the v0.3.7 screenshot
3. Create an image (Image Migration)
4. Restore an image (Restore Image) — with the destructive callout
5. Eject a disk
6. The `.flowimg` file
7. Troubleshooting (table)
8. Safety

Plus: hero (top) and footer (GitHub / Releases / License links).

## File layout

```
site/
  index.html      # all content, semantic HTML
  styles.css      # design tokens + layout + responsive
  assets/
    app-logo.png            # copied from assets/app-logo.png
    screenshot-v0.3.7.png   # copied from assets/screenshots/
.github/workflows/
  pages.yml       # build-free deploy of site/ to GitHub Pages
```

Scroll-spy is a small inline `IntersectionObserver` script in `index.html` — no
external JS.

## Deployment

- Workflow `pages.yml` triggers on push to `main` (paths: `site/**`).
- Uses `actions/upload-pages-artifact` + `actions/deploy-pages` with the
  `site/` directory as the artifact root.
- After first deploy, enable Pages in repo settings (Source: GitHub Actions).
- v1 URL: `https://dhanabhon.github.io/flow-clone/`. Custom domain deferred.

## Out of scope (v1)

- Thai translation / i18n
- Multi-page navigation
- Search
- Dark theme toggle on the site (app-matching light theme only)

## Future (noted, not built)

- Thai version: add `index.th.html` + a language toggle, or migrate to an SSG
  with i18n if the guide grows.
