import { create } from "zustand";
import type { DiskInfo, Progress } from "@/lib/types";

/**
 * Central flow store. Drives the four-screen flow described in DESIGN.md:
 * home → confirmation → cloning → completed. Holds the selected source/target
 * and the live progress snapshot. All mutations are local UI state — actual
 * cloning is initiated through src/lib/tauri.ts.
 */

export type FlowPhase = "home" | "confirmation" | "cloning" | "completed";

interface FlowState {
  phase: FlowPhase;
  source: DiskInfo | null;
  target: DiskInfo | null;
  jobId: string | null;
  progress: Progress | null;
  verify: boolean;

  setSource: (d: DiskInfo | null) => void;
  setTarget: (d: DiskInfo | null) => void;
  setVerify: (v: boolean) => void;
  goTo: (p: FlowPhase) => void;
  beginClone: (jobId: string) => void;
  setProgress: (p: Progress) => void;
  reset: () => void;
}

export const useFlowStore = create<FlowState>((set) => ({
  phase: "home",
  source: null,
  target: null,
  jobId: null,
  progress: null,
  verify: true,

  setSource: (d) => set({ source: d }),
  setTarget: (d) => set({ target: d }),
  setVerify: (v) => set({ verify: v }),
  goTo: (p) => set({ phase: p }),
  beginClone: (jobId) => set({ phase: "cloning", jobId }),
  setProgress: (p) => set({ progress: p }),
  reset: () =>
    set({ phase: "home", source: null, target: null, jobId: null, progress: null }),
}));
