import { create } from "zustand";

export type Theme = "light" | "dark";

const STORAGE_KEY = "flowclone-theme";

/** First-run default: dark mode first. */
function initialTheme(): Theme {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // localStorage may be unavailable in some sandboxed contexts; ignore.
  }
  return "dark";
}

interface ThemeState {
  theme: Theme;
  /** Set an explicit theme and persist it. */
  setTheme: (t: Theme) => void;
  /** Flip between light and dark. */
  toggle: () => void;
}

/** Global theme store. Owns the source of truth and persists the choice. */
export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: initialTheme(),
  setTheme: (t) => {
    try {
      localStorage.setItem(STORAGE_KEY, t);
    } catch {
      // Ignore write failures; in-memory state still updates.
    }
    document.documentElement.classList.toggle("dark", t === "dark");
    set({ theme: t });
  },
  toggle: () => get().setTheme(get().theme === "dark" ? "light" : "dark"),
}));
