/**
 * clone-flow feature.
 *
 * Confirmation: a modal sheet summarizing source/target, requiring the user to
 * type ERASE before enabling the clone button (see DESIGN.md).
 * Cloning: full-screen circular progress with speed/ETA/elapsed and the
 * source → target flow animation.
 *
 * The current screen implementations live in src/routes/index.tsx and wire
 * into the core via `startCloneStub` / `onProgress` in lib/tauri.ts.
 */
export {};
