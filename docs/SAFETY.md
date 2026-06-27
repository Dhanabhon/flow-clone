# FlowClone - Safety Model

FlowClone does not erase disks in Phase 1. Disk detection uses read-only macOS
`diskutil` metadata by default; the deterministic mock catalog is still
available for demos and tests. Restore, direct clone writes, and verification
are mocked. Image creation is read-only, but macOS still requires elevated raw
disk access to read `/dev/rdiskN`.

## Principles

1. **Safety over features.** If a feature cannot be made safe, it ships later.
2. **Make unsafe states unrepresentable.** Validation runs in the core, not the
   UI; the UI cannot bypass it.
3. **Resolve at action time.** The core resolves device paths against the
   current catalog before a workflow starts.
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

The disk catalog can flag a boot device (`is_boot`). The UI disables selecting
it as a target. The macOS catalog parses boot metadata from `diskutil info
-plist`; core-level real boot-disk enforcement belongs with the future
destructive-write safety gates.

## Cancellation

Cancellation is cooperative. `CloneJob` holds an `AtomicBool` cancel token and
the stub raw engine checks it between progress ticks. Real cancellation policy
must be revisited before real writes ship.

## Verification

Phase 1 verification returns a mocked pass result. The blockwise SHA-256
sampler exists for later, but the default verifier does not open devices.

## Privileged helper

Raw reads and writes to `/dev/rdiskN` require root/admin authorization on macOS.
The production privileged helper (`native/macos-helper`) is not implemented yet.
Until then, raw image testing uses the CLI under `sudo`; the desktop GUI must
not be run as root.

## What is explicitly out of scope

- Protecting against a user who deliberately selects the wrong target and types
  ERASE. FlowClone maximizes confidence; it cannot read intent.
- Physical damage to hardware caused by the selected operation.
- Issues on unsupported or end-of-life OS versions.

## Reporting

Every completed job can generate a report preview and export it as a Markdown
file. Restore and direct clone reports still describe mocked workflows until
real write paths exist.
