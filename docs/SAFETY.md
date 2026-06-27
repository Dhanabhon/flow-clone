# FlowClone — Safety Model

FlowClone erases disks. Every design decision below exists to prevent writing
to the wrong device or destroying data the user did not intend to destroy.

## Principles

1. **Safety over features.** If a feature cannot be made safe, it ships later.
2. **Make unsafe states unrepresentable.** Validation runs in the core, not the
   UI; the UI cannot bypass it.
3. **Re-read at action time.** Disk metadata captured in the UI is stale by the
   time a clone starts. The core resolves device paths against a fresh catalog
   read immediately before writing (TOCTOU guard).
4. **Confirm before every destructive step.** The confirmation screen requires
   typed confirmation ("ERASE") and shows source/target serials and capacities.

## Hard validation guards (`CloneRequest::validate`)

A clone request is rejected if:

- Source and target are the same device (`SameDevice`).
- Target capacity is smaller than source (`TargetTooSmall`).
- Source or target device path is not present in the current catalog
  (`SourceNotFound` / `TargetNotFound`).

These are enforced **in Rust**, not TypeScript, so they cannot be skipped by a
UI bug or a modified client.

## Boot disk protection

The disk catalog flags the current boot device (`is_boot`). The UI disables
selecting it as a target. The core additionally refuses to proceed if the boot
device is the resolved target.

## Cancellation

Cancellation is cooperative. `CloneJob` holds an `AtomicBool` cancel token; the
raw engine checks it between blocks. The UI exposes Cancel only **before writing
begins** (per DESIGN.md) — once data is flowing, cancelling mid-write could
leave the target in a corrupted state, so the operation runs to completion or
fails.

## Verification

After a raw clone, `flowclone-verify` hashes source and target block-by-block
(SHA-256) and compares digests. A mismatch fails the job and is surfaced in the
report. Verification is on by default.

## Privileged helper

Raw writes to `/dev/rdiskN` require root/admin authorization on macOS. The
privileged helper (`native/macos-helper`) is the **only** component that holds
that capability. It accepts a narrow, versioned IPC contract and refuses
anything else. See [`../native/macos-helper`](../native/macos-helper).

## What is explicitly out of scope

- Protecting against a user who deliberately selects the wrong target and types
  ERASE. FlowClone maximizes confidence; it cannot read intent.
- Physical damage to hardware caused by the selected operation.
- Issues on unsupported or end-of-life OS versions.

## Reporting

Every completed job can emit a Markdown and JSON report (source, target,
capacity, average speed, duration, verification result, warnings, app version,
timestamp) so the outcome is auditable after the fact.
