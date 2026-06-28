import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DiskInfo, ImageValidation, Progress } from "./types";
import { fileNameFromPath } from "./utils";

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
let browserCancelRequested = false;

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
    : Promise.resolve(restoreBrowserImageStub(imagePath, targetPath));

export const validateImageStub = (
  imagePath: string
): Promise<ImageValidation> =>
  isTauriRuntime()
    ? invoke<ImageValidation>("validate_image_stub", { imagePath })
    : Promise.resolve(validateBrowserImageStub(imagePath));

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
        imagePath
          ? `# FlowClone image migration report\n\n- Source: ${sourcePath}\n- Image: ${imagePath}\n- Mode: Image Migration preview\n- Result: completed\n- Restore: ready for a future target SSD\n`
          : `# FlowClone direct clone report\n\n- Source: ${sourcePath}\n- Target: ${
              targetPath ?? "none"
            }\n- Mode: Direct Clone preview\n- Result: completed\n`
      );

export async function saveReportFile(text: string): Promise<string | null> {
  const defaultPath = `flowclone-report-${new Date().toISOString().slice(0, 10)}.md`;

  if (isTauriRuntime()) {
    const path = await save({
      defaultPath,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!path) return null;
    await writeTextFile(path, text);
    return path;
  }

  const url = URL.createObjectURL(new Blob([text], { type: "text/markdown" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = defaultPath;
  anchor.click();
  URL.revokeObjectURL(url);
  return defaultPath;
}

export const cancelClone = (): Promise<void> => {
  if (isTauriRuntime()) return invoke("cancel_clone");
  browserCancelRequested = true;
  return Promise.resolve();
};

export function openFullDiskAccessSettings(): Promise<void> {
  if (isTauriRuntime()) return invoke("open_full_disk_access_settings");
  window.location.assign(
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
  );
  return Promise.resolve();
}

/** An image job that was interrupted by a crash or power loss. */
export interface PendingImage {
  image_path: string;
  source_model: string;
  bytes_done: number;
  total_bytes: number;
}

export const pendingImageJob = (): Promise<PendingImage | null> =>
  isTauriRuntime()
    ? invoke<PendingImage | null>("pending_image_job")
    : Promise.resolve(null);

export const discardPendingImage = (): Promise<void> =>
  isTauriRuntime() ? invoke("discard_pending_image") : Promise.resolve();

export const dismissPendingImage = (): Promise<void> =>
  isTauriRuntime() ? invoke("dismiss_pending_image") : Promise.resolve();

export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  document.body.append(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) throw new Error("Clipboard is not available.");
}

/** Subscribe to clone progress events emitted by the core. */
export function onProgress(cb: (p: Progress) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    browserProgress.add(cb);
    return Promise.resolve(() => browserProgress.delete(cb));
  }
  return listen<Progress>("clone://progress", (e) => cb(e.payload));
}

/**
 * Subscribe to disk attach/detach events from the native watcher. Lets the UI
 * refresh the disk list on change instead of polling. No-op in the browser.
 */
export function onDisksChanged(cb: () => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return Promise.resolve(() => {});
  return listen("disks://changed", () => cb());
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
  const jobId = `image-${Date.now()}`;
  const total = source.total_bytes;
  browserCancelRequested = false;

  Array.from({ length: 10 }, (_, i) => i + 1).forEach((step) => {
    window.setTimeout(() => {
      if (browserCancelRequested) return;
      const fraction = step / 10;
      if (step === 10) {
        downloadBrowserImageStub(imagePath, source);
      }
      emitBrowserProgress({
        job_id: jobId,
        phase: step === 10 ? "completed" : "cloning",
        fraction,
        bytes_done: Math.round(total * fraction),
        bytes_total: total,
        read_speed: 520_000_000,
        write_speed: 480_000_000,
        elapsed_secs: step * 0.18,
        eta_secs: step === 10 ? 0 : (10 - step) * 0.18,
        current_operation:
          step === 10
            ? `Image workflow ready at ${imagePath}`
            : `Creating image block ${step} to ${imagePath}`,
      });
    }, step * 180);
  });

  return jobId;
}

function downloadBrowserImageStub(imagePath: string, source: DiskInfo) {
  const contents = JSON.stringify(
    {
      format: "flowclone-stub-image",
      version: 1,
      source,
      note: "Preview file only. No disk data has been copied.",
    },
    null,
    2
  );
  const url = URL.createObjectURL(
    new Blob([contents], { type: "application/json" })
  );
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileNameFromPath(imagePath, "migration.flowimg");
  anchor.click();
  URL.revokeObjectURL(url);
}

function validateBrowserImageStub(imagePath: string): ImageValidation {
  if (!imagePath.trim()) throw new Error("image path is required");
  return {
    format: "flowclone-stub-image",
    version: 1,
    source: browserDisks()[0],
    payload_bytes: 0,
    note: "Preview file only. No disk data has been copied.",
  };
}

function restoreBrowserImageStub(imagePath: string, targetPath: string): string {
  validateBrowserImageStub(imagePath);
  const target = browserDisks().find((disk) => disk.device_path === targetPath);
  if (!target) throw new Error(`target not found: ${targetPath}`);
  return `restore-${Date.now()}`;
}
