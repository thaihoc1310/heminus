import { expect, it } from "vitest";
import { ShellPromptState } from "./terminalShellIntegration";

it("tracks only commands entered at a confirmed shell prompt, not TUI input", () => {
  const state = new ShellPromptState();
  expect(state.atPrompt).toBe(false);
  state.marker("D;0");
  state.submit("herdr");
  expect(state.atPrompt).toBe(false);
  expect(state.editable).toBe(false);
  for (let i = 0; i < 10000; i++) state.submit("private agent prompt");
  expect(state.marker("D;0")).toBe("herdr");
  expect(state.marker("D;0")).toBeNull();
  state.submit("false");
  expect(state.marker("D;1")).toBeNull();
});

it("stays editable for shells that never report prompt markers", () => {
  const state = new ShellPromptState();
  expect(state.editable).toBe(true);
  state.submit("ls");
  expect(state.editable).toBe(true);
  state.marker("C");
  expect(state.editable).toBe(false);
});

it("recognises multiplexers and mouse reports", async () => {
  const { startsMultiplexer, isMouseReport, isMousePress } = await import("./terminalShellIntegration");
  expect(startsMultiplexer("herdr")).toBe(true);
  expect(startsMultiplexer(" /usr/bin/tmux attach")).toBe(true);
  expect(startsMultiplexer("herdrx")).toBe(false);
  expect(isMouseReport("\x1b[<35;10;4M\x1b[<35;11;4M")).toBe(true);
  expect(isMouseReport("pi")).toBe(false);
  expect(isMousePress("\x1b[<0;10;4M")).toBe(true);
  expect(isMousePress("\x1b[<35;10;4M")).toBe(false);
  expect(isMousePress("\x1b[<64;10;4M")).toBe(false);
  expect(isMousePress("\x1b[<0;10;4m")).toBe(false);
});

it("recognises password and one-time code prompts", async () => {
  const { isSecretPrompt } = await import("./terminalShellIntegration");
  expect(isSecretPrompt("[sudo] password for thaihoc: ")).toBe(true);
  expect(isSecretPrompt("Enter passphrase for key '/home/u/.ssh/id_ed25519':")).toBe(true);
  expect(isSecretPrompt("Verification code:")).toBe(true);
  expect(isSecretPrompt("thaihoc@ubuntu:~$ echo password")).toBe(false);
});
