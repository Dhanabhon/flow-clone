/**
 * clone-flow feature — owns the Confirmation and Cloning screens.
 *
 * Confirmation: a modal sheet summarizing source/target, requiring the user to
 * type ERASE before enabling the clone button (see DESIGN.md).
 * Cloning: full-screen circular progress with speed/ETA/elapsed and the
 * source → target flow animation.
 *
 * Wires into the core via `startClone` / `onProgress` in lib/tauri.ts.
 */
export {};
