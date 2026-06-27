# Security Policy

FlowClone performs **destructive disk operations**. Security and safety are
non-negotiable. See [`docs/SAFETY.md`](docs/SAFETY.md) for the full safety and
threat model.

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities.

Email: security@flowclone.local (replace with real address before release).

Include:

- A description of the issue and its impact
- Steps to reproduce
- Affected version(s)

We will acknowledge within 72 hours and aim for a fix within 30 days.

## Scope

In scope:

- Bypassing confirmation guards before destructive writes
- Writing to the wrong device (target confusion / TOCTOU)
- Privilege escalation via the privileged helper
- Crashes that corrupt in-progress clones

Out of scope:

- Physical hardware damage from user-selected targets
- Issues on unsupported OS versions

## Privileged operations

Raw disk access on macOS requires elevated privileges. These operations are
isolated in `native/macos-helper` and communicate with the app over a strict,
audited IPC contract. The main app **never** shells out to arbitrary commands.
