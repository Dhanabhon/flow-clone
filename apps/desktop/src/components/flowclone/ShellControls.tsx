import { LanguageToggle } from "@/components/flowclone/LanguageToggle";
import { ThemeToggle } from "@/components/flowclone/ThemeToggle";

export function ShellControls() {
  return (
    <div
      aria-label="Display controls"
      className="fixed left-1/2 top-5 z-50 flex h-11 -translate-x-1/2 items-center gap-2 rounded-pill bg-[#f5f7fb]/95 px-2 shadow-[0_8px_24px_rgba(15,23,42,0.12)] ring-1 ring-inset ring-[#d6dde8] backdrop-blur dark:bg-surface/80 dark:ring-border dark:shadow-soft sm:left-auto sm:right-5 sm:translate-x-0"
      role="group"
    >
      <ThemeToggle />
      <div className="h-5 w-px bg-[#d6dde8] dark:bg-border" />
      <LanguageToggle />
    </div>
  );
}
