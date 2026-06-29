# First-run Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a skippable, re-openable 4-step onboarding the first time FlowClone launches, ending with the macOS Full Disk Access grant (or a UAC note on Windows).

**Architecture:** A self-contained overlay (`<Onboarding/>`) gated by a new Zustand store (`onboarding-store`), rendered above the routes in `App.tsx`. It does not touch the workflow phase machine (`flow-store`) or the create/restore paths. Spec: `docs/superpowers/specs/2026-06-30-onboarding-design.md`.

**Tech Stack:** React + TypeScript, Zustand (manual-localStorage pattern), Tailwind, lucide-react, existing `useI18n` and `openFullDiskAccessSettings`.

## Global Constraints

- **No frontend test runner** (per `CLAUDE.md`). Each task is gated by `pnpm typecheck` (expect `Done`, no errors) and `pnpm --filter desktop lint` (expect no output = clean). UI behavior is verified by a manual run in the final task.
- **i18n keys must exist in BOTH `en` and `th`** in `apps/desktop/src/lib/i18n.ts`. `th` is typed `Record<MessageKey, string>`, so a missing Thai key is a typecheck error.
- Zustand stores follow the **manual-localStorage** pattern of `theme-store.ts` (try/catch around `localStorage`, in-memory state still updates on failure).
- Components are PascalCase; hooks/util modules lowercase. Do **not** modify `flow-store.ts` or any create/restore code.
- macOS is primary; Windows only changes the step-3 copy (detected via `navigator.userAgent`).

---

### Task 1: Onboarding store

**Files:**
- Create: `apps/desktop/src/stores/onboarding-store.ts`

**Interfaces:**
- Produces: `useOnboardingStore` — Zustand hook with state `{ hasSeen: boolean; open: boolean }` and actions `finish(): void`, `reopen(): void`. `open` initializes to `!hasSeen`. `finish()` persists seen + closes; `reopen()` sets `open=true`.

- [ ] **Step 1: Create the store**

```typescript
import { create } from "zustand";

const STORAGE_KEY = "flowclone-onboarding-seen";

/** Whether onboarding has been finished or skipped on a previous launch. */
function initialSeen(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    // localStorage may be unavailable in some sandboxed contexts; ignore.
    return false;
  }
}

interface OnboardingState {
  /** True once the user has finished or skipped onboarding at least once. */
  hasSeen: boolean;
  /** Whether the onboarding overlay is currently shown. */
  open: boolean;
  /** Finish or skip: persist that it has been seen and close the overlay. */
  finish: () => void;
  /** Show the onboarding overlay again (e.g. from the toolbar). */
  reopen: () => void;
}

/** First-run onboarding store. Independent of the workflow phase machine. */
export const useOnboardingStore = create<OnboardingState>((set) => {
  const seen = initialSeen();
  return {
    hasSeen: seen,
    open: !seen,
    finish: () => {
      try {
        localStorage.setItem(STORAGE_KEY, "1");
      } catch {
        // Ignore write failures; in-memory state still updates.
      }
      set({ hasSeen: true, open: false });
    },
    reopen: () => set({ open: true }),
  };
});
```

- [ ] **Step 2: Typecheck**

Run: `pnpm typecheck`
Expected: `apps/desktop typecheck: Done` with no errors.

- [ ] **Step 3: Lint**

Run: `pnpm --filter desktop lint`
Expected: no output (clean).

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/stores/onboarding-store.ts
git commit -m "feat(onboarding): add onboarding store (first-run flag + open state)"
```

---

### Task 2: i18n copy (English + Thai)

**Files:**
- Modify: `apps/desktop/src/lib/i18n.ts` (add keys to the `en` object near the other groups; the `th: Record<MessageKey, string>` object must get the same keys)

**Interfaces:**
- Produces these `MessageKey`s (used by Tasks 3–4): `onboardingSkip`, `onboardingBack`, `onboardingContinue`, `onboardingGetStarted`, `onboardingReopen`, `onboardingStepLabel`, `onboardingWelcomeTitle`, `onboardingWelcomeBody`, `onboardingWhatTitle`, `onboardingWhatBody`, `onboardingPermTitleMac`, `onboardingPermBodyMac`, `onboardingPermOpen`, `onboardingPermTitleWin`, `onboardingPermBodyWin`, `onboardingReadyTitle`, `onboardingReadyBody`. `onboardingStepLabel` interpolates `{current}` and `{total}`.

- [ ] **Step 1: Add the keys to the `en` object**

Add this block inside the `en = { ... }` object (anywhere among the existing keys, e.g. right before its closing brace):

```typescript
  // Onboarding (first-run guide)
  onboardingSkip: "Skip",
  onboardingBack: "Back",
  onboardingContinue: "Continue",
  onboardingGetStarted: "Get started",
  onboardingReopen: "Show the welcome guide",
  onboardingStepLabel: "Step {current} of {total}",
  onboardingWelcomeTitle: "Welcome to FlowClone",
  onboardingWelcomeBody:
    "Move everything, lose nothing. Let's get you set up in a few seconds.",
  onboardingWhatTitle: "What FlowClone does",
  onboardingWhatBody:
    "Image Migration copies an SSD into a single `.flowimg` file — pick Smart (used data only), Exact (full copy), and optional compression. Restore Image writes that file back onto another SSD. Restore erases the target, so always double-check the disk.",
  onboardingPermTitleMac: "Grant Full Disk Access",
  onboardingPermBodyMac:
    "FlowClone needs Full Disk Access to read and write raw disks. Open System Settings, turn on FlowClone, then quit and reopen the app.",
  onboardingPermOpen: "Open Full Disk Access",
  onboardingPermTitleWin: "About permissions",
  onboardingPermBodyWin:
    "On Windows, FlowClone asks for permission with a UAC prompt when you create or restore an image — nothing to set up here.",
  onboardingReadyTitle: "You're all set",
  onboardingReadyBody:
    "Plug in a drive to begin. You can reopen this guide anytime from the toolbar.",
```

- [ ] **Step 2: Add the matching keys to the `th` object**

Add this block inside the `th: Record<MessageKey, string> = { ... }` object:

```typescript
  // Onboarding (first-run guide)
  onboardingSkip: "ข้าม",
  onboardingBack: "ย้อนกลับ",
  onboardingContinue: "ถัดไป",
  onboardingGetStarted: "เริ่มใช้งาน",
  onboardingReopen: "แสดงคู่มือเริ่มต้นอีกครั้ง",
  onboardingStepLabel: "ขั้นที่ {current} จาก {total}",
  onboardingWelcomeTitle: "ยินดีต้อนรับสู่ FlowClone",
  onboardingWelcomeBody:
    "ย้ายทุกอย่าง ไม่สูญหาย — ตั้งค่าให้พร้อมใช้ในไม่กี่วินาที",
  onboardingWhatTitle: "FlowClone ทำอะไรได้บ้าง",
  onboardingWhatBody:
    "Image Migration คัดลอก SSD เป็นไฟล์ `.flowimg` ไฟล์เดียว — เลือก Smart (เฉพาะข้อมูลที่ใช้), Exact (ก๊อปเต็ม) และบีบอัดได้ ส่วน Restore Image เขียนไฟล์นั้นกลับลง SSD อีกลูก การกู้คืนจะลบข้อมูลดิสก์ปลายทาง ตรวจให้แน่ใจทุกครั้ง",
  onboardingPermTitleMac: "เปิดสิทธิ์ Full Disk Access",
  onboardingPermBodyMac:
    "FlowClone ต้องใช้สิทธิ์ Full Disk Access เพื่ออ่าน/เขียนดิสก์แบบ raw เปิด System Settings แล้วเปิดสวิตช์ให้ FlowClone จากนั้นปิดและเปิดแอปใหม่",
  onboardingPermOpen: "เปิด Full Disk Access",
  onboardingPermTitleWin: "เกี่ยวกับสิทธิ์การเข้าถึง",
  onboardingPermBodyWin:
    "บน Windows FlowClone จะขอสิทธิ์ผ่าน UAC prompt ตอนสร้างหรือกู้คืนอิมเมจ ไม่ต้องตั้งค่าล่วงหน้า",
  onboardingReadyTitle: "พร้อมแล้ว",
  onboardingReadyBody:
    "เสียบไดรฟ์เพื่อเริ่มต้น เปิดคู่มือนี้ดูซ้ำได้ตลอดจากแถบเครื่องมือ",
```

- [ ] **Step 3: Typecheck (this proves both locales have every key)**

Run: `pnpm typecheck`
Expected: `Done`. If a key is missing from `th`, typecheck fails with a "missing properties" error on the `th` object — add it.

- [ ] **Step 4: Lint**

Run: `pnpm --filter desktop lint`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/lib/i18n.ts
git commit -m "feat(onboarding): add onboarding copy (EN + TH)"
```

---

### Task 3: Onboarding overlay component

**Files:**
- Create: `apps/desktop/src/features/onboarding/Onboarding.tsx`

**Interfaces:**
- Consumes: `useOnboardingStore` (Task 1) → `finish`; `useI18n().t` with the keys from Task 2; `openFullDiskAccessSettings` from `@/lib/tauri`; `appLogo` from `@/assets/app-logo.png`; `Button` from `@/components/ui/button`.
- Produces: `Onboarding` — a default-styled full-screen overlay component, used by Task 4.

- [ ] **Step 1: Create the component**

```tsx
import { useState } from "react";
import type { ReactNode } from "react";
import { HardDriveUpload, Lock, Rocket } from "lucide-react";
import appLogo from "@/assets/app-logo.png";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { openFullDiskAccessSettings } from "@/lib/tauri";
import { useOnboardingStore } from "@/stores/onboarding-store";

const STEP_COUNT = 4;

/** True when running on Windows — only changes the permissions-step copy. */
function isWindows(): boolean {
  try {
    return /win/i.test(navigator.userAgent);
  } catch {
    return false;
  }
}

/**
 * First-run onboarding overlay: a self-contained 4-step tour ending with the
 * permission the app needs. Independent of the workflow phase machine; shown
 * whenever the onboarding store's `open` is true.
 */
export function Onboarding() {
  const { t } = useI18n();
  const finish = useOnboardingStore((s) => s.finish);
  const [step, setStep] = useState(0);
  const [accessError, setAccessError] = useState<string | null>(null);
  const windows = isWindows();
  const isLast = step === STEP_COUNT - 1;

  async function openAccess() {
    setAccessError(null);
    try {
      await openFullDiskAccessSettings();
    } catch (err) {
      setAccessError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-background p-6">
      <section className="relative w-full max-w-lg rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <button
          type="button"
          onClick={finish}
          className="absolute right-5 top-5 text-sm font-medium text-muted transition hover:text-text"
        >
          {t("onboardingSkip")}
        </button>

        <p className="text-xs uppercase tracking-wide text-muted">
          {t("onboardingStepLabel", { current: step + 1, total: STEP_COUNT })}
        </p>

        {step === 0 && (
          <div className="mt-5">
            <img
              src={appLogo}
              alt="FlowClone"
              className="mx-auto h-16 w-16 rounded-2xl"
            />
            <h2 className="mt-5 text-2xl font-semibold">
              {t("onboardingWelcomeTitle")}
            </h2>
            <p className="mx-auto mt-3 max-w-sm text-sm text-muted">
              {t("onboardingWelcomeBody")}
            </p>
          </div>
        )}

        {step === 1 && (
          <StepBody
            icon={<HardDriveUpload className="h-8 w-8" />}
            title={t("onboardingWhatTitle")}
            body={t("onboardingWhatBody")}
          />
        )}

        {step === 2 && (
          <div className="mt-5">
            <Badge icon={<Lock className="h-8 w-8" />} />
            <h2 className="mt-5 text-2xl font-semibold">
              {t(windows ? "onboardingPermTitleWin" : "onboardingPermTitleMac")}
            </h2>
            <p className="mx-auto mt-3 max-w-sm text-sm text-muted">
              {t(windows ? "onboardingPermBodyWin" : "onboardingPermBodyMac")}
            </p>
            {!windows && (
              <Button className="mt-5" variant="secondary" onClick={openAccess}>
                {t("onboardingPermOpen")}
              </Button>
            )}
            {accessError && (
              <p className="mt-3 text-sm text-danger">{accessError}</p>
            )}
          </div>
        )}

        {step === 3 && (
          <StepBody
            icon={<Rocket className="h-8 w-8" />}
            title={t("onboardingReadyTitle")}
            body={t("onboardingReadyBody")}
          />
        )}

        <div className="mt-8 flex items-center justify-between">
          <Button
            variant="secondary"
            className={step === 0 ? "invisible" : ""}
            onClick={() => setStep((s) => Math.max(0, s - 1))}
          >
            {t("onboardingBack")}
          </Button>

          <div className="flex items-center gap-1.5">
            {Array.from({ length: STEP_COUNT }).map((_, i) => (
              <span
                key={i}
                className={
                  i === step
                    ? "h-2 w-4 rounded-full bg-primary"
                    : "h-2 w-2 rounded-full bg-border"
                }
              />
            ))}
          </div>

          <Button onClick={() => (isLast ? finish() : setStep((s) => s + 1))}>
            {isLast ? t("onboardingGetStarted") : t("onboardingContinue")}
          </Button>
        </div>
      </section>
    </div>
  );
}

function Badge({ icon }: { icon: ReactNode }) {
  return (
    <div className="mx-auto grid h-16 w-16 place-items-center rounded-full bg-primary/15 text-primary">
      {icon}
    </div>
  );
}

function StepBody({
  icon,
  title,
  body,
}: {
  icon: ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="mt-5">
      <Badge icon={icon} />
      <h2 className="mt-5 text-2xl font-semibold">{title}</h2>
      <p className="mx-auto mt-3 max-w-sm text-sm text-muted">{body}</p>
    </div>
  );
}
```

- [ ] **Step 2: Typecheck**

Run: `pnpm typecheck`
Expected: `Done`. (Confirms the i18n keys, store action, and `Button`/icon imports all resolve.)

- [ ] **Step 3: Lint**

Run: `pnpm --filter desktop lint`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/features/onboarding/Onboarding.tsx
git commit -m "feat(onboarding): add 4-step onboarding overlay component"
```

---

### Task 4: Wire into the app shell (render overlay + re-open button)

**Files:**
- Modify: `apps/desktop/src/app/App.tsx`
- Modify: `apps/desktop/src/components/flowclone/ShellControls.tsx`

**Interfaces:**
- Consumes: `Onboarding` (Task 3), `useOnboardingStore` (Task 1) → `open` and `reopen`, `useI18n().t` → `onboardingReopen`.

- [ ] **Step 1: Render the overlay in `App.tsx`**

Add the imports near the other `@/` imports:

```tsx
import { Onboarding } from "@/features/onboarding/Onboarding";
import { useOnboardingStore } from "@/stores/onboarding-store";
```

Inside `App()`, add the subscription with the other store hooks (near `const theme = ...`):

```tsx
  const onboardingOpen = useOnboardingStore((s) => s.open);
```

Change the returned JSX from:

```tsx
    <QueryClientProvider client={queryClient}>
      <ShellControls />
      <Routes />
    </QueryClientProvider>
```

to:

```tsx
    <QueryClientProvider client={queryClient}>
      <ShellControls />
      <Routes />
      {onboardingOpen && <Onboarding />}
    </QueryClientProvider>
```

- [ ] **Step 2: Add the re-open button in `ShellControls.tsx`**

Replace the whole file with:

```tsx
import { HelpCircle } from "lucide-react";
import { LanguageToggle } from "@/components/flowclone/LanguageToggle";
import { ThemeToggle } from "@/components/flowclone/ThemeToggle";
import { useI18n } from "@/lib/i18n";
import { useOnboardingStore } from "@/stores/onboarding-store";

export function ShellControls() {
  const { t } = useI18n();
  const reopen = useOnboardingStore((s) => s.reopen);
  return (
    <div
      aria-label="Display controls"
      className="fixed left-1/2 top-5 z-50 flex h-11 -translate-x-1/2 items-center gap-2 rounded-pill bg-[#f5f7fb]/95 px-2 shadow-[0_8px_24px_rgba(15,23,42,0.12)] ring-1 ring-inset ring-[#d6dde8] backdrop-blur sm:left-auto sm:right-5 sm:translate-x-0"
      role="group"
    >
      <ThemeToggle />
      <div className="h-5 w-px bg-[#d6dde8]" />
      <LanguageToggle />
      <div className="h-5 w-px bg-[#d6dde8]" />
      <button
        type="button"
        onClick={reopen}
        aria-label={t("onboardingReopen")}
        title={t("onboardingReopen")}
        className="grid h-8 w-8 shrink-0 place-items-center rounded-pill text-slate-500 transition hover:bg-black/5 hover:text-slate-800"
      >
        <HelpCircle className="h-[18px] w-[18px]" strokeWidth={2} />
      </button>
    </div>
  );
}
```

- [ ] **Step 3: Typecheck**

Run: `pnpm typecheck`
Expected: `Done`.

- [ ] **Step 4: Lint**

Run: `pnpm --filter desktop lint`
Expected: clean.

- [ ] **Step 5: Manual verification (no automated UI tests in this project)**

Run the app with the mock backend so no real disks are needed:

```bash
FLOWCLONE_DISK_BACKEND=mock pnpm dev
```

Verify, in a browser at the dev URL (or via `pnpm tauri dev`):
- First load shows the overlay (clear `localStorage` key `flowclone-onboarding-seen` if it was already set: in devtools, `localStorage.removeItem("flowclone-onboarding-seen")`, then reload).
- Continue moves through all 4 steps; Back returns; the dots track the step; Back is hidden on step 1.
- Step 3 shows "Grant Full Disk Access" + the "Open Full Disk Access" button on macOS.
- "Get started" (step 4) and "Skip" (any step) both close the overlay and it does **not** reappear on reload.
- The toolbar help (?) button reopens it.
- Toggle dark mode and Thai — both render correctly.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/app/App.tsx apps/desktop/src/components/flowclone/ShellControls.tsx
git commit -m "feat(onboarding): show overlay on first run + toolbar re-open button"
```

---

## Notes

- Detailed images/cloning are untouched; this is purely additive UI + a new store.
- No CHANGELOG/version bump in this plan — fold that into a release commit when cutting the next version.
