import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useEffect } from "react";
import { Routes } from "@/routes";
import { ShellControls } from "@/components/flowclone/ShellControls";
import { useLocaleStore } from "@/stores/locale-store";
import { useThemeStore } from "@/stores/theme-store";

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false } },
});

/**
 * App shell. The UI is a presenter only — all cloning happens in the Rust
 * core via Tauri commands. See `src/lib/tauri.ts`.
 */
export default function App() {
  // Apply the persisted theme on mount. The store's setter keeps the DOM
  // in sync thereafter; this effect handles the first paint and any later
  // OS-prefers change only when the user hasn't made an explicit choice.
  const theme = useThemeStore((s) => s.theme);
  const locale = useLocaleStore((s) => s.locale);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  return (
    <QueryClientProvider client={queryClient}>
      <ShellControls />
      <Routes />
    </QueryClientProvider>
  );
}
