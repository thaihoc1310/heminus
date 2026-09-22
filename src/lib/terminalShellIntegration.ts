/** Track one shell command, never the input of programs launched by it. */
export class ShellPromptState {
  atPrompt = false;
  /** Set by the first marker. SSH hosts and non-bash shells never send one. */
  private integrated = false;
  private pending: string | null = null;

  /** Whether typed input is plausibly a shell command line. */
  get editable(): boolean {
    return this.atPrompt || !this.integrated;
  }

  marker(data: string): string | null {
    this.integrated = true;
    const [kind, status] = data.split(";");
    if (kind === "C") this.atPrompt = false;
    if (kind === "A" || kind === "D") this.atPrompt = true;
    if (kind !== "D") return null;
    const command = status === "0" ? this.pending : null;
    this.pending = null;
    return command;
  }

  submit(command: string) {
    if (!this.atPrompt) return;
    this.pending = command.trim() || null;
    this.atPrompt = false;
  }
}

const multiplexers = new Set(["herdr", "tmux", "zellij", "screen", "byobu"]);

/** Whether a command typed at the shell prompt starts a terminal multiplexer. */
export function startsMultiplexer(command: string): boolean {
  const program = command.trim().split(/\s+/)[0]?.split("/").pop() ?? "";
  return multiplexers.has(program);
}

/** A mouse report from xterm: SGR (`ESC[<b;x;yM`) or X10 (`ESC[M` + 3 bytes). */
export function isMouseReport(data: string): boolean {
  return /^(?:\x1b\[<\d+;\d+;\d+[Mm]|\x1b\[M[\s\S]{3})+$/.test(data);
}

/** A button press, which in a multiplexer may move focus to another pane. */
export function isMousePress(data: string): boolean {
  const match = /^\x1b\[<(\d+);\d+;\d+M$/.exec(data);
  return match !== null && (Number(match[1]) & (32 | 64)) === 0;
}

/** The text before the cursor asks for a secret that the terminal will not echo. */
export function isSecretPrompt(line: string): boolean {
  return /(?:password|passphrase|verification code|one-time code|otp|token)[^:\n]{0,40}:\s*$/i.test(line);
}
