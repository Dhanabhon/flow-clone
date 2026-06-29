import { create } from "zustand";
import type { DiskInfo, Progress } from "@/lib/types";

/**
 * Central flow store. Drives the four-screen flow described in DESIGN.md:
 * home → confirmation → cloning → completed. Holds the selected source/target
 * and the live progress snapshot. All mutations are local UI state — actual
 * cloning is initiated through src/lib/tauri.ts.
 */

export type FlowPhase = "home" | "confirmation" | "cloning" | "completed";
export type WorkflowMode = "clone" | "image" | "restore";

interface FlowState {
  phase: FlowPhase;
  mode: WorkflowMode;
  source: DiskInfo | null;
  target: DiskInfo | null;
  imagePath: string | null;
  /** Image Migration: store only used blocks (Smart) vs a full copy (Exact). */
  imageUsedOnly: boolean;
  /** Image Migration: write a zstd-compressed `.flowimg`. */
  imageCompress: boolean;
  jobId: string | null;
  progress: Progress | null;
  verify: boolean;
  report: string | null;

  setMode: (mode: WorkflowMode) => void;
  setSource: (d: DiskInfo | null) => void;
  setTarget: (d: DiskInfo | null) => void;
  setImagePath: (path: string | null) => void;
  setImageUsedOnly: (v: boolean) => void;
  setImageCompress: (v: boolean) => void;
  setVerify: (v: boolean) => void;
  setReport: (report: string | null) => void;
  goTo: (p: FlowPhase) => void;
  beginClone: (jobId: string, mode?: WorkflowMode) => void;
  setProgress: (p: Progress) => void;
  reset: () => void;
}

export const useFlowStore = create<FlowState>((set) => ({
  phase: "home",
  mode: "image",
  source: null,
  target: null,
  imagePath: null,
  imageUsedOnly: false,
  imageCompress: false,
  jobId: null,
  progress: null,
  verify: true,
  report: null,

  setMode: (mode) =>
    set((state) => ({
      mode,
      source: mode === "restore" ? null : state.source,
      target: mode === "image" ? null : state.target,
      imagePath: state.mode === mode ? state.imagePath : null,
    })),
  setSource: (d) => set({ source: d }),
  setTarget: (d) => set({ target: d }),
  setImagePath: (path) => set({ imagePath: path }),
  setImageUsedOnly: (v) => set({ imageUsedOnly: v }),
  setImageCompress: (v) => set({ imageCompress: v }),
  setVerify: (v) => set({ verify: v }),
  setReport: (report) => set({ report }),
  goTo: (p) => set({ phase: p }),
  beginClone: (jobId, mode = "clone") =>
    set({ phase: "cloning", jobId, mode, progress: null, report: null }),
  setProgress: (p) => set({ progress: p }),
  reset: () =>
    set({
      phase: "home",
      mode: "image",
      source: null,
      target: null,
      imagePath: null,
      imageUsedOnly: false,
      imageCompress: false,
      jobId: null,
      progress: null,
      report: null,
    }),
}));
