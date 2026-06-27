import { create } from "zustand";

export type Locale = "en" | "th";

const STORAGE_KEY = "flowclone-locale";

function initialLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "th") return saved;
    if (navigator.language.toLowerCase().startsWith("th")) return "th";
  } catch {
    // Ignore unavailable browser APIs; English is the stable fallback.
  }
  return "en";
}

interface LocaleState {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  toggle: () => void;
}

export const useLocaleStore = create<LocaleState>((set, get) => ({
  locale: initialLocale(),
  setLocale: (locale) => {
    try {
      localStorage.setItem(STORAGE_KEY, locale);
    } catch {
      // In-memory state still updates.
    }
    document.documentElement.lang = locale;
    set({ locale });
  },
  toggle: () => get().setLocale(get().locale === "en" ? "th" : "en"),
}));
