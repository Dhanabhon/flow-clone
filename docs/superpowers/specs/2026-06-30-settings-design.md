# Design — Settings panel (+ onboarding becomes first-run only)

Date: 2026-06-30
Status: Approved (pending spec review)

## Goal

Add an in-app **Settings** modal, reached from a gear button in the toolbar. It
hosts appearance, the macOS Full Disk Access setup, and an About section (version +
"Report an issue"). At the same time, drop the onboarding **re-open** affordance —
onboarding becomes first-run only, and the permission help it offered now lives
permanently in Settings.

## Decisions (from discussion)

- **v1 sections:** Appearance, Permissions, About. (Image defaults, save-folder,
  notifications toggle are deferred to a later iteration.)
- **Form factor:** a centered, dismissible **modal** (close via an X button, a
  backdrop click, or Esc), scrollable — not a full-screen takeover.
- **Onboarding is first-run only.** Remove the `reopen` action and the toolbar
  "Show the welcome guide" button. The first-run flag (localStorage) is unchanged.
- **FDA lives in Settings**, since it's a recurring need (re-grant after some
  updates, accidental toggle-off) and is where users look after a "Disk Access
  Required" failure.
- **About → Report an issue** links to `https://github.com/Dhanabhon/flow-clone/issues`.
- External links open through a new, scheme-validated Rust command (mirrors the
  existing `open_full_disk_access_settings`) rather than the shell plugin directly.

## Architecture (overlay + store, mirrors onboarding)

Settings is independent of the workflow phase machine. A small store holds the
open/closed UI state; the modal renders above the routes in `App.tsx`.

### Components / files

- **NEW `stores/settings-store.ts`** — Zustand store: `{ open: boolean;
  openSettings(): void; closeSettings(): void }`. UI state only — **not
  persisted** (v1 has no stored preferences; theme/language already persist via
  their own stores).
- **NEW `features/settings/Settings.tsx`** — the modal. Renders a backdrop +
  centered card with the three sections. Closes on the X button, a backdrop
  click, and the `Escape` key (a `keydown` listener added/removed in a
  `useEffect`). Scrollable body (`max-h` + `overflow-y-auto`) for small windows.
- **EDIT `components/flowclone/ShellControls.tsx`** — replace the `HelpCircle`
  ("Show the welcome guide") button with a `Settings` (gear) button that calls
  `useSettingsStore().openSettings`, labeled `t("settingsOpen")`.
- **EDIT `stores/onboarding-store.ts`** — remove the `reopen` action and the
  `OnboardingState.reopen` field (onboarding is first-run only now).
- **EDIT `apps/desktop/src-tauri/src/commands.rs`** — add command
  `open_external_url(url: String) -> Result<(), String>`. It rejects any URL whose
  scheme is not `http://` or `https://` (guards against command injection / opening
  arbitrary schemes), then opens it the same way `open_full_disk_access_settings`
  opens a URL/pane (macOS `open`, Windows the equivalent). Factor the check into a
  pure `fn is_allowed_external_url(url: &str) -> bool` so it is unit-testable.
- **EDIT `apps/desktop/src-tauri/src/lib.rs`** — register `open_external_url` in
  `generate_handler![...]`.
- **EDIT `apps/desktop/src/lib/tauri.ts`** — add `openExternal(url: string):
  Promise<void>` — `invoke("open_external_url", { url })` under Tauri, else
  `window.open(url, "_blank")` in the browser fallback.
- **EDIT `apps/desktop/src/lib/i18n.ts`** — Settings copy in English and Thai.

## The Settings modal sections

1. **Appearance** — Theme (light/dark) and Language (EN/ไทย). Reuse the existing
   `ThemeToggle` and `LanguageToggle` components (driven by `theme-store` /
   `locale-store`), each under a labeled row in this section.
2. **Permissions (adaptive by OS, via `navigator.userAgent`)**
   - **macOS:** explains Full Disk Access; an **"Open Full Disk Access"** button
     calling `openFullDiskAccessSettings()`; the "after enabling, quit & reopen"
     note. On command error, show an inline error line; the modal stays usable.
   - **Windows:** a short UAC note; no button.
3. **About** — app name, **version** via `getVersion()` from `@tauri-apps/api/app`
   (loaded in a `useEffect`, guarded by `isTauriRuntime()`; omitted if
   unavailable), a **"Report an issue"** action →
   `https://github.com/Dhanabhon/flow-clone/issues`, and a **GitHub** link →
   `https://github.com/Dhanabhon/flow-clone`, both via `openExternal(...)`.

## Behavior

- Toolbar gear → `openSettings()` → modal shows. Close via X / backdrop / Esc →
  `closeSettings()`.
- Onboarding: first-run only. No re-open path remains (toolbar button and
  `reopen` removed). The first-run localStorage flag and the onboarding flow are
  otherwise unchanged.
- Settings does not touch `flow-store` or the create/restore paths.

## Styling

Centered modal: a fixed full-viewport backdrop (`bg-black/50`) with a centered
`rounded-card bg-surface shadow-soft` panel, consistent with the interrupt modal.
Sections separated by subtle dividers/headings. Honors light/dark and the current
language.

## Error handling

- `openFullDiskAccessSettings()` and `openExternal()` may reject → caught, shown
  as an inline error within the relevant section; the modal stays open and usable.
- `getVersion()` failure (e.g., browser fallback) → version simply not shown.
- The `open_external_url` command returns an error for non-http(s) URLs; the
  frontend only ever passes the two known https links, but the guard is defense in
  depth.

## Out of scope (v1)

- Image-migration defaults (Smart/Exact, Compress, save folder, verify) and a
  notifications toggle — deferred to a later iteration; they need persisted prefs
  and flow-store seeding.
- Native `Cmd+,` Settings menu item.
- Safety invariants (typed `ERASE`, target validation, free-space check) remain
  mandatory and are **never** exposed as settings.

## Testing / verification

No frontend test runner (per `CLAUDE.md`) → verify with `pnpm typecheck`,
`pnpm --filter desktop lint`, and a manual run. **Rust:** add a `#[cfg(test)]`
unit test for `is_allowed_external_url` — accepts `http://`/`https://`, rejects
`file://`, `javascript:`, shell metacharacters, and empty input. Manual checks:
gear opens Settings; X/backdrop/Esc close it; the FDA button opens System Settings
(macOS); Report an issue / GitHub open in the browser; light/dark + EN/TH render;
onboarding no longer has a re-open button and still shows once on first run.
