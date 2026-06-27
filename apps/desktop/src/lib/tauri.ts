import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DiskInfo, Progress } from "./types";

/**
 * Strongly-typed wrappers around the Tauri commands defined in
 * apps/desktop/src-tauri/src/commands.rs. The UI only ever talks to the core
 * through these — it never clones directly.
 */

export const listDisks = (): Promise<DiskInfo[]> => invoke("list_disks");

export const startClone = (
  sourcePath: string,
  targetPath: string,
  verify: boolean
): Promise<string> =>
  invoke<string>("start_clone", {
    sourcePath,
    targetPath,
    verify,
  });

export const cancelClone = (): Promise<void> => invoke("cancel_clone");

/** Subscribe to clone progress events emitted by the core. */
export function onProgress(cb: (p: Progress) => void): Promise<UnlistenFn> {
  return listen<Progress>("clone://progress", (e) => cb(e.payload));
}
