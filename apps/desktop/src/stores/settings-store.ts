import { create } from "zustand";

interface SettingsState {
  /** Whether the Settings modal is currently shown. */
  open: boolean;
  /** Open the Settings modal. */
  openSettings: () => void;
  /** Close the Settings modal. */
  closeSettings: () => void;
}

/**
 * Settings modal UI store. Transient, UI-only state — nothing here is
 * persisted. Theme and language preferences live in their own stores
 * (`theme-store`, `locale-store`); Settings only reuses their toggles.
 */
export const useSettingsStore = create<SettingsState>((set) => ({
  open: false,
  openSettings: () => set({ open: true }),
  closeSettings: () => set({ open: false }),
}));
