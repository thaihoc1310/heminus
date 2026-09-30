/**
 * Ctrl+Shift+<key> chords the window handles (App.svelte), by physical key.
 * A terminal pane must keep them from the shell: xterm sends Ctrl+Shift+X as
 * Ctrl+X, so Ctrl+Shift+T would transpose two characters, Ctrl+Shift+J press
 * Enter, and so on.
 */
export const appCtrlShiftCodes = new Set([
  "KeyT", // new terminal
  "KeyN", // new-tab page
  "KeyE", // Move to…
  "KeyB", // broadcast input
  "KeyH", // focus the pane to the left
  "KeyJ", // … below
  "KeyK", // … above
  "KeyL" // … to the right
]);
