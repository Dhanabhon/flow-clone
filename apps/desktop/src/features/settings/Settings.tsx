import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { Github, X } from "lucide-react";
import { LanguageToggle } from "@/components/flowclone/LanguageToggle";
import { ThemeToggle } from "@/components/flowclone/ThemeToggle";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import {
  isTauriRuntime,
  openExternal,
  openFullDiskAccessSettings,
} from "@/lib/tauri";
import { useSettingsStore } from "@/stores/settings-store";

const ISSUES_URL = "https://github.com/Dhanabhon/flow-clone/issues";
const REPO_URL = "https://github.com/Dhanabhon/flow-clone";

/** True when running on Windows — only changes the permissions-section copy. */
function isWindows(): boolean {
  try {
    return /win/i.test(navigator.userAgent);
  } catch {
    return false;
  }
}

/**
 * Settings modal: Appearance, Permissions, and About. Independent of the
 * workflow phase machine; shown whenever the settings store's `open` is true.
 * Closes on the X button, a backdrop click, or the Escape key.
 */
export function Settings() {
  const { t } = useI18n();
  const close = useSettingsStore((s) => s.closeSettings);
  const windows = isWindows();
  const [accessError, setAccessError] = useState<string | null>(null);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);

  // Close on Escape.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [close]);

  // Load the app version (Tauri only; omitted in the browser fallback).
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let active = true;
    import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then((value) => {
        if (active) setVersion(value);
      })
      .catch(() => {
        // Version simply not shown if it can't be read.
      });
    return () => {
      active = false;
    };
  }, []);

  async function openAccess() {
    setAccessError(null);
    try {
      await openFullDiskAccessSettings();
    } catch (err) {
      setAccessError(err instanceof Error ? err.message : String(err));
    }
  }

  async function openLink(url: string) {
    setLinkError(null);
    try {
      await openExternal(url);
    } catch (err) {
      setLinkError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div
      className="fixed inset-0 z-[60] grid place-items-center bg-black/50 p-6"
      onClick={close}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("settingsTitle")}
        onClick={(event) => event.stopPropagation()}
        className="relative flex max-h-[85vh] w-full max-w-md flex-col overflow-hidden rounded-card border border-border bg-surface shadow-soft"
      >
        <header className="flex items-center justify-between border-b border-border px-6 py-4">
          <h2 className="text-lg font-semibold">{t("settingsTitle")}</h2>
          <button
            type="button"
            onClick={close}
            aria-label={t("settingsClose")}
            className="grid h-8 w-8 place-items-center rounded-input text-muted outline-none transition hover:bg-black/5 hover:text-text focus-visible:ring-2 focus-visible:ring-primary"
          >
            <X className="h-[18px] w-[18px]" strokeWidth={2} />
          </button>
        </header>

        <div className="flex-1 space-y-6 overflow-y-auto px-6 py-5">
          <Section title={t("settingsAppearance")}>
            <Row label={t("settingsTheme")}>
              <ThemeToggle />
            </Row>
            <Row label={t("settingsLanguage")}>
              <LanguageToggle />
            </Row>
          </Section>

          <Section title={t("settingsPermissions")}>
            {windows ? (
              <p className="text-sm text-muted">{t("settingsPermBodyWin")}</p>
            ) : (
              <>
                <p className="text-sm text-muted">{t("settingsPermBodyMac")}</p>
                <Button
                  variant="secondary"
                  className="mt-3"
                  onClick={openAccess}
                >
                  {t("settingsPermOpen")}
                </Button>
                {accessError && (
                  <p className="mt-2 text-sm text-danger">{accessError}</p>
                )}
              </>
            )}
          </Section>

          <Section title={t("settingsAbout")}>
            <p className="text-sm font-medium">FlowClone</p>
            {version && (
              <p className="text-sm text-muted">
                {t("settingsVersion", { version })}
              </p>
            )}
            <p className="mt-3 text-sm text-muted">{t("settingsReportBody")}</p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button variant="secondary" onClick={() => openLink(ISSUES_URL)}>
                {t("settingsReportIssue")}
              </Button>
              <Button variant="ghost" onClick={() => openLink(REPO_URL)}>
                <Github className="mr-2 h-4 w-4" strokeWidth={2} />
                {t("settingsViewGithub")}
              </Button>
            </div>
            {linkError && (
              <p className="mt-2 text-sm text-danger">{linkError}</p>
            )}
          </Section>
        </div>
      </section>
    </div>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section>
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-wide text-muted">
        {title}
      </h3>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5">
      <span className="text-sm font-medium">{label}</span>
      {children}
    </div>
  );
}
