# FlowClone macOS Privileged Helper

> ⚠️ **Future phase.** This component is not built yet. It is documented here
> so the security boundary it will enforce is clear from day one.

## Purpose

Raw reads/writes to macOS whole-disk devices (`/dev/rdiskN`) require
authorization that the main FlowClone app does not hold. Rather than running
the whole app as root, the privileged operation is isolated in this helper,
which the app talks to over a strict, versioned IPC contract.

## Why a separate helper

- **Least privilege.** Only this small, audited binary holds disk-write
  capability. The rest of FlowClone runs as the normal user.
- **Auditable surface.** The helper accepts a tiny, well-defined request set and
  refuses anything else.
- **Sandboxing.** The helper can be signed, entitled, and (eventually) sandboxed
  independently of the UI.

## IPC contract (planned)

The helper exposes a single command surface. Each request must include:

| Field | Meaning |
| --- | --- |
| `version` | Contract version; mismatches are rejected |
| `op` | One of `clone`, `verify` |
| `source` | Absolute raw device path |
| `target` | Absolute raw device path |
| `total_bytes` | Byte count to operate on |
| `verify` | Whether to verify after clone |

The helper re-validates every guard from `flowclone-core` independently before
touching a device. It streams progress back over the same channel.

## Installation

A future `scripts/install-helper.sh` will install the helper via
`SMJobBless` / `ServiceManagement`, with `launchd` registration and version
pinning. The installer refuses to downgrade.

## Layout

```
Sources/
  HelperMain.swift     # entry point, authorization check
  IpcContract.swift    # request/response types (mirrors the table above)
  DiskOps.swift        # delegates to a Rust dylib for the actual copy
```

The heavy lifting (raw copy, verify) stays in the Rust crates; the Swift helper
is a thin, privileged boundary that calls into them.
