/** One line of movement per cell. A flick larger than this is clamped. */
const MAX_WHEEL_LINES = 40;

export type TerminalMouseProtocol = "none" | "x10" | "vt200" | "drag" | "any";
export type TerminalMouseEncoding = "default" | "sgr" | "sgr-pixels";

/**
 * Tracks whether the foreground app asked for mouse-wheel reports.
 *
 * Claude Code and Codex scroll their own viewport from those reports. xterm
 * turns a trackpad into at most one report, and only after shrinking the
 * distance by 70%, so the app barely moves. This state lets the pane send one
 * report per row of travel instead.
 */
export class TerminalMouseTracking {
  protocol: TerminalMouseProtocol = "none";
  encoding: TerminalMouseEncoding = "default";
  /** Sub-line distance still waiting for a full row. */
  remainder = 0;

  /** VT200 and newer include the wheel. X10 does not. */
  get reportsWheel(): boolean {
    return this.protocol === "vt200" || this.protocol === "drag" || this.protocol === "any";
  }

  setPrivateMode(mode: number, enabled: boolean): void {
    if (mode === 9 || mode === 1000 || mode === 1002 || mode === 1003) {
      this.remainder = 0;
      if (!enabled) {
        this.protocol = "none";
        return;
      }
      this.protocol =
        mode === 9 ? "x10" : mode === 1000 ? "vt200" : mode === 1002 ? "drag" : "any";
      return;
    }
    if (mode === 1006) this.encoding = enabled ? "sgr" : "default";
    if (mode === 1016) this.encoding = enabled ? "sgr-pixels" : "default";
  }

  reset(): void {
    this.protocol = "none";
    this.encoding = "default";
    this.remainder = 0;
  }

  /**
   * How many wheel reports this event is worth.
   *
   * Pixel deltas map 1:1 onto cell height, with no trackpad penalty. Line
   * deltas are already in rows. The fractional part carries to the next event
   * so a slow swipe still advances.
   */
  consume(deltaY: number, deltaMode: number, cellHeight: number, pageRows: number): number {
    if (!Number.isFinite(deltaY) || deltaY === 0) return 0;
    let amount = deltaY;
    if (deltaMode === 0) {
      const height = cellHeight > 1 ? cellHeight : 16;
      amount = deltaY / height;
    } else if (deltaMode === 2) {
      amount = deltaY * Math.max(1, pageRows);
    }
    this.remainder += amount;
    const lines = Math.trunc(this.remainder) || 0;
    this.remainder -= lines;
    if (lines > MAX_WHEEL_LINES) return MAX_WHEEL_LINES;
    if (lines < -MAX_WHEEL_LINES) return -MAX_WHEEL_LINES;
    return lines;
  }
}

export interface WheelReportPosition {
  col: number;
  row: number;
  x: number;
  y: number;
}

/** Cell under the pointer, using the same origin as xterm's mouse reports. */
export function wheelReportPosition(
  clientX: number,
  clientY: number,
  bounds: { left: number; top: number; width: number; height: number },
  cellWidth: number,
  cellHeight: number
): WheelReportPosition | null {
  if (cellWidth <= 0 || cellHeight <= 0) return null;
  const x = Math.min(Math.max(clientX - bounds.left, 0), Math.max(0, bounds.width - 1));
  const y = Math.min(Math.max(clientY - bounds.top, 0), Math.max(0, bounds.height - 1));
  return {
    col: Math.floor(x / cellWidth),
    row: Math.floor(y / cellHeight),
    x: Math.floor(x),
    y: Math.floor(y)
  };
}

export interface WheelReportInput {
  lines: number;
  position: WheelReportPosition;
  encoding: TerminalMouseEncoding;
  cols: number;
  rows: number;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}

/** SGR or X10 wheel reports. Negative `lines` is wheel-up. Empty when nothing should be sent. */
export function encodeWheelReports(input: WheelReportInput): string {
  const count = Math.abs(input.lines);
  if (count === 0 || input.cols < 1 || input.rows < 1) return "";
  let code = 64 | (input.lines > 0 ? 1 : 0);
  if (input.shift) code += 4;
  if (input.alt) code += 8;
  if (input.ctrl) code += 16;
  const col = Math.min(Math.max(input.position.col + 1, 1), input.cols);
  const row = Math.min(Math.max(input.position.row + 1, 1), input.rows);
  let report: string;
  if (input.encoding === "sgr") {
    report = `\x1b[<${code};${col};${row}M`;
  } else if (input.encoding === "sgr-pixels") {
    report = `\x1b[<${code};${input.position.x};${input.position.y}M`;
  } else {
    const params = [code + 32, col + 32, row + 32];
    if (params.some((value) => value > 255)) return "";
    report = `\x1b[M${String.fromCharCode(params[0], params[1], params[2])}`;
  }
  return report.repeat(count);
}
