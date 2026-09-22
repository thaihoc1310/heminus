export const MIN_TERMINAL_FONT_SIZE = 9;
export const MAX_TERMINAL_FONT_SIZE = 32;
export const DEFAULT_TERMINAL_FONT_SIZE = 14;

export type TerminalZoomAction = "in" | "out" | "reset";

type ZoomKeyEvent = Pick<
  KeyboardEvent,
  "key" | "code" | "ctrlKey" | "metaKey" | "altKey"
>;
type ZoomWheelEvent = Pick<WheelEvent, "ctrlKey" | "metaKey" | "deltaY">;

export function clampTerminalFontSize(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_TERMINAL_FONT_SIZE;
  return Math.max(MIN_TERMINAL_FONT_SIZE, Math.min(MAX_TERMINAL_FONT_SIZE, Math.round(value)));
}

export function nextTerminalFontSize(
  current: number,
  action: TerminalZoomAction
): number {
  if (action === "reset") return DEFAULT_TERMINAL_FONT_SIZE;
  return clampTerminalFontSize(current + (action === "in" ? 1 : -1));
}

/** Ctrl/Cmd + − / + / 0, including the = key and the numpad. */
export function terminalZoomActionFromKeyboard(event: ZoomKeyEvent): TerminalZoomAction | null {
  if ((!event.ctrlKey && !event.metaKey) || event.altKey) return null;
  const key = event.key;
  const code = event.code;
  if (key === "+" || key === "=" || code === "NumpadAdd") return "in";
  // Ctrl+_ (Shift+-) stays with the shell: it is readline's undo.
  if (key === "-" || code === "NumpadSubtract") return "out";
  if (key === "0" || code === "Digit0" || code === "Numpad0") return "reset";
  return null;
}

export function terminalZoomActionFromWheel(event: ZoomWheelEvent): TerminalZoomAction | null {
  if ((!event.ctrlKey && !event.metaKey) || event.deltaY === 0) return null;
  return event.deltaY < 0 ? "in" : "out";
}
