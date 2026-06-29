# Design — First-run Onboarding

Date: 2026-06-30
Status: Approved (pending spec review)

## Goal

Show a short guided onboarding the first time FlowClone launches, ending with the
permission grant the app actually needs (macOS Full Disk Access). It must be
skippable and re-openable later. macOS is the primary target; Windows is handled
with an adaptive note (no FDA there).

## Decisions (from brainstorming)

- **Scope:** a 4-step tour — Welcome → What it does → Permissions → Ready.
- **Permission step:** guide the user, offer an "Open Full Disk Access" button
  (existing `openFullDiskAccessSettings()` command), then a "Continue" button. We
  **trust** the user — macOS gives no reliable way to verify FDA without a
  privileged raw read, and FDA usually needs an app restart to take effect, so
  the step tells the user to quit & reopen.
- **Skippable + re-openable:** auto-shown on first run, "Skip" on every step, and
  re-openable any time from an in-app control.
- **Re-open entry point:** a small button in the existing `ShellControls`
  toolbar (simpler and cross-platform; avoids wiring the native macOS menu +
  events).
- **Adaptive per OS:** macOS shows the FDA step; Windows shows a UAC note instead.

## Architecture (approach A — isolated overlay + store)

Onboarding is **independent of** the workflow phase machine (`flow-store`). It is
a self-contained overlay gated by its own store, rendered above the routes.

```
App.tsx
 ├─ <ShellControls/>            (+ "Show onboarding" button → reopen())
 ├─ {open && <Onboarding/>}     (full-screen overlay above the routes)
 └─ <Routes/>                   (unchanged workflow screens)
```

### Components / files

- **NEW `stores/onboarding-store.ts`** — Zustand store, same manual-localStorage
  pattern as `theme-store.ts`.
  - State: `hasSeen: boolean` (persisted), `open: boolean` (runtime; initialized
    to `!hasSeen`).
  - Actions: `finish()` → set `hasSeen=true`, persist, `open=false`; `reopen()` →
    `open=true`. `skip()` is an alias of `finish()`.
  - localStorage key: `"flowclone-onboarding-seen"` (value `"1"`). Reads/writes
    wrapped in try/catch — on failure, default `hasSeen=false` (shows once per
    session), matching `theme-store`'s graceful handling.
- **NEW `features/onboarding/Onboarding.tsx`** — the overlay. Owns local `step`
  state (0–3), renders the current step's content, and the nav controls. On the
  last step's primary button or any "Skip", calls the store action.
  - Step content defined as a small in-file array/records (title, body, icon,
    optional action) so steps are data, not branching markup.
- **EDIT `app/App.tsx`** — subscribe to `open`; render `<Onboarding/>` above
  `<Routes/>` when `open` is true.
- **EDIT `components/flowclone/ShellControls.tsx`** — add a Help/onboarding icon
  button (lucide, e.g. `HelpCircle`) that calls `reopen()`.
- **EDIT `lib/i18n.ts`** — all onboarding copy in English and Thai.

## The four steps

1. **Welcome** — app logo, name, tagline ("Move everything. Lose nothing.") and a
   one-line intro.
2. **What it does** — Image Migration (Smart / Exact / Compress) and Restore Image
   in plain terms, plus a short note that Restore erases the target.
3. **Permissions (adaptive)**
   - **macOS:** explains Full Disk Access; **"Open Full Disk Access"** button
     (calls `openFullDiskAccessSettings()`); note "after enabling, quit & reopen
     FlowClone"; **Continue** advances. On command error, show an inline error
     line (mirrors the existing permission-denied card).
   - **Windows:** text — "FlowClone asks for permission with a UAC prompt when you
     create or restore an image; no setup needed here." **Continue** advances.
     (No "Open settings" button.)
4. **Ready** — short recap, **"Get started"** primary button → `finish()`.

## Platform detection

Detect via `navigator.userAgent` in the webview (no new dependency):
`/Mac/i` → macOS, `/Win/i` → Windows, otherwise default to the macOS copy
(primary platform). This only switches the step-3 copy/buttons; nothing
security-sensitive depends on it.

## Behavior

- **First run:** `hasSeen` is false → `open` initializes true → overlay shows.
- **Skip:** a "Skip" affordance (top-right) on every step → `finish()`.
- **Navigation:** Back (hidden on step 0) and Continue; a 4-dot step indicator.
  The Permissions step's Continue does not require FDA to be granted.
- **Re-open:** ShellControls button → `reopen()` → overlay shows again; finishing
  or skipping closes it. `hasSeen` stays true.
- **Does not touch** `flow-store`; the underlying screen (usually Home) stays
  mounted behind the overlay.

## Styling

Full-screen, matching the app's existing screens: a centered `rounded-card
bg-surface shadow-soft` panel on the page background, primary CTA in the accent
color, muted secondary text, step dots. Light and dark themes both honored
(uses the existing color tokens). Respects the current language.

## Error handling

- `openFullDiskAccessSettings()` may reject → catch and show an inline error in
  the Permissions step; the user can still Continue.
- localStorage unavailable → treated as "not seen" (shows once per session); no
  crash.

## Testing / verification

The project has no frontend test runner (per `CLAUDE.md`), so verification is
`pnpm typecheck`, `pnpm lint`, and a manual run. The store's logic is small and
pure (flag flip + persistence); the overlay is presentational. Manual checks:
first-run shows; Skip and Get started both set `hasSeen` and close; re-open
works; macOS vs Windows copy switches; light/dark and EN/TH render.

## Out of scope

- Programmatically verifying or granting Full Disk Access (not possible without a
  privileged raw read; intentionally trust + "quit & reopen").
- Native macOS menu item for re-open (using an in-app button instead).
- Any change to the workflow phase machine or the create/restore paths.
