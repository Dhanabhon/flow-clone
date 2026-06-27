import { useEffect, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import { localeOptions, useI18n } from "@/lib/i18n";
import { useLocaleStore, type Locale } from "@/stores/locale-store";
import { cn } from "@/lib/utils";

export function LanguageToggle() {
  const { locale, t } = useI18n();
  const setLocale = useLocaleStore((state) => state.setLocale);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const active = localeOptions.find((option) => option.value === locale)!;

  useEffect(() => {
    if (!open) return;

    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };

    document.addEventListener("pointerdown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-label={t("langToggle")}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((next) => !next)}
        className="flex h-8 w-[132px] items-center justify-between rounded-pill px-3 text-sm font-semibold text-text transition hover:bg-white/70 focus:outline-none focus:ring-2 focus:ring-primary dark:hover:bg-elevated"
      >
        <span className="flex items-center gap-2">
          <FlagIcon locale={active.value} />
          {active.label}
        </span>
        <ChevronDown
          className={cn(
            "h-4 w-4 text-muted transition-transform",
            open && "rotate-180"
          )}
          strokeWidth={2}
        />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute right-0 mt-2 w-[148px] overflow-hidden rounded-card bg-[#f5f7fb] p-1 shadow-[0_12px_28px_rgba(15,23,42,0.18)] ring-1 ring-inset ring-[#d6dde8] dark:bg-surface dark:ring-border"
        >
          {localeOptions.map((option) => (
            <button
              key={option.value}
              type="button"
              role="menuitemradio"
              aria-checked={locale === option.value}
              onClick={() => {
                setLocale(option.value);
                setOpen(false);
              }}
              className={cn(
                "flex h-8 w-full items-center gap-2 rounded-button px-3 text-left text-sm font-medium transition",
                locale === option.value
                  ? "bg-primary text-white"
                  : "text-text hover:bg-elevated"
              )}
            >
              <FlagIcon locale={option.value} />
              {option.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function FlagIcon({ locale }: { locale: Locale }) {
  if (locale === "th") {
    return (
      <svg aria-hidden="true" viewBox="0 0 24 16" className="h-4 w-6 rounded-[2px]">
        <rect width="24" height="16" fill="#A51931" />
        <rect y="3" width="24" height="10" fill="#F4F5F8" />
        <rect y="5" width="24" height="6" fill="#2D2A4A" />
      </svg>
    );
  }

  return (
    <svg aria-hidden="true" viewBox="0 0 24 16" className="h-4 w-6 rounded-[2px]">
      <rect width="24" height="16" fill="#fff" />
      <path
        fill="#B22234"
        d="M0 0h24v1.23H0zm0 2.46h24v1.23H0zm0 2.46h24v1.23H0zm0 2.46h24v1.23H0zm0 2.46h24v1.23H0zm0 2.46h24v1.23H0zm0 2.46h24V16H0z"
      />
      <rect width="10.8" height="8.6" fill="#3C3B6E" />
    </svg>
  );
}
