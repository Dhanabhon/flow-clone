import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DiskInfo, Progress } from "./types";

/**
 * Strongly-typed wrappers around the Tauri commands defined in
 * apps/desktop/src-tauri/src/commands.rs. The UI only ever talks to the core
 * through these — it never clones directly.
 */

const mockDisks: DiskInfo[] = [
  {
    device_path: "/dev/disk4",
    bsd_name: "disk4",
    model: "Samsung 970 EVO Plus",
    vendor: "Samsung",
    serial: "S5H9NX0R123456",
    total_bytes: 512_000_000_000,
    used_bytes: 412_000_000_000,
    connection: "usb",
    filesystem: "APFS",
    read_only: false,
    encrypted: false,
    health: "healthy",
    is_boot: false,
    volume_name: "Macintosh Clone",
  },
  {
    device_path: "/dev/disk5",
    bsd_name: "disk5",
    model: "Kingston NV3",
    vendor: "Kingston",
    serial: "50026B7784A2F3D1",
    total_bytes: 1_000_000_000_000,
    used_bytes: 0,
    connection: "usb",
    filesystem: null,
    read_only: false,
    encrypted: false,
    health: "healthy",
    is_boot: false,
    volume_name: "New SSD",
  },
];

const browserProgress = new Set<(p: Progress) => void>();

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function browserDisks(): DiskInfo[] {
  const env = (import.meta as unknown as { env?: Record<string, string | undefined> }).env;
  let localMode: string | null = null;
  try {
    localMode = localStorage.getItem("flowclone-mock-disks");
  } catch {
    localMode = null;
  }
  return env?.VITE_FLOWCLONE_MOCK_DISKS === "one" || localMode === "one"
    ? mockDisks.slice(0, 1)
    : mockDisks;
}

function emitBrowserProgress(progress: Progress) {
  browserProgress.forEach((cb) => cb(progress));
}

export const listDisks = (): Promise<DiskInfo[]> =>
  isTauriRuntime() ? invoke("list_disks") : Promise.resolve(browserDisks());

export const validateClonePlan = (
  sourcePath: string,
  targetPath: string,
  verify: boolean
): Promise<void> =>
  isTauriRuntime()
    ? invoke("validate_clone_plan", {
        sourcePath,
        targetPath,
        verify,
      })
    : Promise.resolve(validateBrowserPlan(sourcePath, targetPath));

export const startCloneStub = (
  sourcePath: string,
  targetPath: string,
  verify: boolean
): Promise<string> =>
  isTauriRuntime()
    ? invoke<string>("start_clone_stub", {
        sourcePath,
        targetPath,
        verify,
      })
    : startBrowserCloneStub(sourcePath, targetPath);

export const createImageStub = (
  sourcePath: string,
  imagePath: string
): Promise<string> =>
  isTauriRuntime()
    ? invoke<string>("create_image_stub", {
        sourcePath,
        imagePath,
      })
    : Promise.resolve(createBrowserImageStub(sourcePath, imagePath));

export const restoreImageStub = (
  imagePath: string,
  targetPath: string
): Promise<string> =>
  isTauriRuntime()
    ? invoke<string>("restore_image_stub", {
        imagePath,
        targetPath,
      })
    : Promise.resolve(`restore-${Date.now()}`);

export const generateReportStub = (
  sourcePath: string,
  targetPath?: string,
  imagePath?: string
): Promise<string> =>
  isTauriRuntime()
    ? invoke<string>("generate_report_stub", {
        sourcePath,
        targetPath,
        imagePath,
      })
    : Promise.resolve(
        `# FlowClone report\n\n- Source: ${sourcePath}\n- Target: ${
          targetPath ?? "none"
        }\n- Image: ${imagePath ?? "none"}\n- Mode: mocked Phase 1 workflow\n`
      );

export const cancelClone = (): Promise<void> => invoke("cancel_clone");

/** Subscribe to clone progress events emitted by the core. */
export function onProgress(cb: (p: Progress) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    browserProgress.add(cb);
    return Promise.resolve(() => browserProgress.delete(cb));
  }
  return listen<Progress>("clone://progress", (e) => cb(e.payload));
}

function validateBrowserPlan(sourcePath: string, targetPath: string) {
  const source = browserDisks().find((disk) => disk.device_path === sourcePath);
  const target = browserDisks().find((disk) => disk.device_path === targetPath);
  if (!source) throw new Error(`source not found: ${sourcePath}`);
  if (!target) throw new Error(`target not found: ${targetPath}`);
  if (source.device_path === target.device_path) throw new Error("same device");
  if (target.total_bytes < source.total_bytes) throw new Error("target too small");
}

function startBrowserCloneStub(sourcePath: string, targetPath: string): Promise<string> {
  validateBrowserPlan(sourcePath, targetPath);
  const source = browserDisks().find((disk) => disk.device_path === sourcePath)!;
  const jobId = `job-${Date.now()}`;
  const total = source.total_bytes;

  Array.from({ length: 12 }, (_, i) => i + 1).forEach((step) => {
    window.setTimeout(() => {
      const fraction = step / 12;
      emitBrowserProgress({
        job_id: jobId,
        phase: step === 12 ? "completed" : "cloning",
        fraction,
        bytes_done: Math.round(total * fraction),
        bytes_total: total,
        read_speed: 825_000_000,
        write_speed: 812_000_000,
        elapsed_secs: step * 0.4,
        eta_secs: step === 12 ? 0 : (12 - step) * 0.4,
        current_operation:
          step === 12 ? "Completed" : `Copying mock block ${step} to ${targetPath}`,
      });
    }, step * 120);
  });

  return Promise.resolve(jobId);
}

function createBrowserImageStub(sourcePath: string, imagePath: string): string {
  const source = browserDisks().find((disk) => disk.device_path === sourcePath);
  if (!source) throw new Error(`source not found: ${sourcePath}`);
  if (!imagePath.trim()) throw new Error("image path is required");
  return `image-${Date.now()}`;
}
