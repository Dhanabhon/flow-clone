FlowClone — DESIGN.md

Version: 0.1.0 MVP
Product: FlowClone
Platform: macOS (Primary)
Design Language: Native macOS + Minimal + Safety First

⸻

Design Philosophy

FlowClone is not a disk management utility.

FlowClone is a single-purpose cloning application.

The user should be able to complete the entire cloning workflow in less than 3 clicks.

Everything in the interface must increase user confidence before destructive operations.

Design principles:

* Safety over features
* Clarity over density
* Confidence over speed
* Native over custom
* Minimal over complicated

⸻

UX Goals

The user should always know:

* Which disk is the source
* Which disk is the target
* What will happen next
* Whether cloning is safe
* Current progress
* Estimated remaining time

The interface must never feel technical.

Avoid Linux-like or Disk Utility-like complexity.

⸻

Design Keywords

* Apple
* Raycast
* CleanShot X
* Arc Browser
* Linear
* Calm
* Premium
* Spacious
* Modern

⸻

Window

Default size

1200 × 780

Resizable

Minimum:

1000 × 680

Centered on startup.

Sized so the 1100px max content width is fully usable with side padding.

⸻

Navigation

No sidebar.

No menu pages.

No tabs.

The application contains only four screens.

1. Home
2. Confirmation
3. Cloning
4. Completed

Settings should live inside a small Preferences window, not inside the main workflow.

⸻

Color System

Light

Background

#F7F8FA

Surface

#FFFFFF

Border

#E5E7EB

Primary

#3B82F6

Success

#22C55E

Warning

#F59E0B

Danger

#EF4444

Text

#111827

Secondary Text

#6B7280

⸻

Dark

Background

#0F1115

Surface

#171A21

Elevated Surface

#1D212A

Border

#2A2F39

Primary

#4F8CFF

Success

#34D399

Warning

#FBBF24

Danger

#F87171

Text

#F9FAFB

Secondary Text

#9CA3AF

⸻

Radius

Cards

20px

Buttons

14px

Inputs

12px

Progress

999px

⸻

Shadows

Soft.

Never use heavy shadows.

Glass blur only where necessary.

⸻

Typography

SF Pro Display

Large Title

36

Title

28

Heading

22

Body

16

Caption

13

Numbers should use tabular figures.

⸻

Icon Style

Use Lucide icons.

No filled icons.

2px stroke.

⸻

Layout

Maximum content width

1100px

Spacing system

8

16

24

32

48

64

⸻

Screen 1

Home

Hero

FlowClone

Safe SSD Cloning

Clone your disk with confidence.

⸻

Below Hero

Two large cards.

Source

↓

Target

Cards should be visually equal.

⸻

Each disk card contains

Disk Icon

Disk Name

Capacity

Connection Type

Filesystem

Serial Number

Health

Used Capacity

Progress Bar

⸻

Example

Samsung 990 Pro

512 GB

APFS

USB 10Gbps

Healthy

412 GB Used

⸻

Between cards

Animated Flow Arrow

Source

────▶

Target

The arrow becomes animated after both disks are selected.

⸻

Primary Button

Start Clone

Full Width

Disabled until:

Source selected

Target selected

Source != Target

Target size >= Source size

⸻

Warning Banner

Target disk will be completely erased.

Amber background.

Visible only after target selection.

⸻

Disk Cards

Cards should feel physical.

Hover

Elevation increases slightly.

Selected

Blue outline

Soft glow

Healthy

Green badge

Read Only

Blue badge

Encrypted

Purple badge

Unknown

Gray badge

⸻

Confirmation Screen

Presented as a modal sheet.

Not a browser dialog.

Sections

Source

↓

Target

Capacity

Serial Numbers

Warning

Type ERASE to continue.

Clone button remains disabled until correct input.

⸻

Cloning Screen

Full screen state.

Everything else disappears.

Large circular progress indicator.

Center aligned.

Progress

61%

⸻

Below

Reading Speed

Writing Speed

Average Speed

Elapsed

Remaining

Current Block

Verification Status

⸻

Flow Animation

Data particles should visually move

Source

────▶▶▶▶

Target

Very subtle.

No flashy animation.

⸻

Buttons

Cancel

Only available before writing begins.

After writing starts

No cancel.

⸻

Verification

After clone finishes

Transition automatically

Clone Completed

↓

Verifying

Separate progress indicator.

Verification icon

Shield

⸻

Success Screen

Large Green Check

Clone Completed

Summary Card

Source

Target

Elapsed

Average Speed

Verification

Buttons

Export Report

Done

⸻

Motion

Use Framer Motion.

Duration

150~250ms

Easing

easeOut

Avoid bouncing animations.

Only meaningful transitions.

⸻

Sound

Optional.

One soft success sound.

No error sounds.

⸻

Accessibility

Keyboard first.

Tab navigation.

Visible focus states.

Minimum contrast AA.

Screen reader labels.

⸻

Empty State

No drives connected.

Large illustration.

Connect your SSDs to begin.

Automatically refresh every second.

⸻

Error States

Target too small

Red banner

Target disk is smaller than source disk.

⸻

Same Disk Selected

Cannot clone to the same device.

⸻

Disk Removed

Pause animation.

Display reconnect instructions.

⸻

Read Failure

Display

Retry

Abort

⸻

Progress Design

Never display only percentage.

Always display

Progress

Speed

ETA

Elapsed

Current Operation

Users trust progress more when numbers are visible.

⸻

Report

Export JSON

Export Markdown

Include

Source

Target

Capacity

Average Speed

Duration

Verification Result

Warnings

Application Version

Timestamp

⸻

Future Features

Image File Mode

Clone Image

Restore Image

Queue

SMART

Disk Health

Resume Clone

Multi Verify

Network Clone

⸻

Tech Stack

Same stack as FlowFTP

* Tauri v2
* Rust
* React
* TypeScript
* Tailwind CSS
* shadcn/ui
* Framer Motion
* Lucide Icons
* TanStack Query (if needed)
* Zustand
* React Hook Form
* Zod

⸻

UX Principle

The application should feel like a premium macOS utility created by Apple.

Every screen must answer one question only.

Never overwhelm the user.

Every destructive action must increase confidence before execution.

FlowClone should feel calm, trustworthy, and invisible.