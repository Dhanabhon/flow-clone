/**
 * Shared FlowClone types.
 *
 * Hand-mirrored from the Rust crates until `ts-rs` codegen is wired up.
 * @see packages/shared-types/README.md
 */

export type Connection =
  | "unknown"
  | "internal"
  | "usb"
  | "thunderbolt"
  | "firewire"
  | "network";

export type Health = "unknown" | "healthy" | "warning" | "failing";

export interface DiskInfo {
  device_path: string;
  bsd_name: string;
  model: string;
  vendor: string | null;
  serial: string | null;
  total_bytes: number;
  used_bytes: number | null;
  connection: Connection;
  filesystem: string | null;
  read_only: boolean;
  encrypted: boolean;
  health: Health;
  is_boot: boolean;
  volume_name: string | null;
}

export type Phase =
  | "preparing"
  | "cloning"
  | "verifying"
  | "completed"
  | "failed";

export interface Progress {
  job_id: string;
  phase: Phase;
  fraction: number;
  bytes_done: number;
  bytes_total: number;
  read_speed: number;
  write_speed: number;
  elapsed_secs: number;
  eta_secs: number | null;
  current_operation: string;
}
