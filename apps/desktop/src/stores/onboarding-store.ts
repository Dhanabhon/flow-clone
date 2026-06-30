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
  };
});
