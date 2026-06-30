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
