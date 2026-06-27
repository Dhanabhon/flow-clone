import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** shadcn-style className combiner. */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

/** Format bytes as a short human string, e.g. "412 GB". */
export function formatBytes(bytes: number): string {
  const units: [string, number][] = [
    ["TB", 1e12],
    ["GB", 1e9],
    ["MB", 1e6],
    ["KB", 1e3],
  ];
  for (const [unit, scale] of units) {
    if (bytes >= scale) return `${(bytes / scale).toFixed(bytes >= scale * 10 ? 0 : 1)} ${unit}`;
  }
  return `${bytes} B`;
}

/** Format a bytes/sec rate. */
export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

/** Format seconds as e.g. "3m 12s". */
export function formatDuration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m === 0) return `${r}s`;
  return `${m}m ${r}s`;
}
