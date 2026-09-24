const delay = (ms) => new Promise((r) => setTimeout(r, ms));
for (let i = 0; i < 100 && !window.smoke; i++) await delay(100);
if (!window.smoke) return "FAIL harness did not mount";
const out = [];
const check = (name, ok, extra = "") => out.push(`${ok ? "PASS" : "FAIL"} ${name} ${extra}`);
const calls = [];
const original = window.__TAURI_INTERNALS__.invoke;
window.__TAURI_INTERNALS__.invoke = (cmd, args, opts) => { calls.push({ cmd, args }); return original(cmd, args, opts); };
const textarea = document.querySelector(".xterm-helper-textarea");
textarea.focus();
const type = (data) => textarea.dispatchEvent(new InputEvent("input", { data, inputType: "insertText", composed: false }));
const key = (key, keyCode) => textarea.dispatchEvent(new KeyboardEvent("keydown", { key, keyCode, bubbles: true, cancelable: true }));

check("webgl renderer active", Boolean(document.querySelector(".xterm-screen canvas")));
const beforeFocusKey = smoke.text().length;
key("F11", 122); await delay(30);
check("F11 toggles focus without reaching the shell", smoke.focusToggles.length === 1 && smoke.text().length === beforeFocusKey);

// A shell with no prompt markers (SSH host, zsh) still gets suggestions.
smoke.emit("$ ");
await delay(50);
type("pi"); await delay(80);
check("suggestions without shell markers", Boolean(document.querySelector(".terminal-suggestions")));

// Password prompts don't echo: nothing typed there may reach the panel.
key("Escape", 27); await delay(20);
smoke.emit("\r\n[sudo] password for thaihoc: "); await delay(40);
type("pi"); await delay(80);
check("no suggestions at a password prompt", !document.querySelector(".terminal-suggestions"));
smoke.emit("\r\n$ "); await delay(40);
type("pi"); await delay(80);

// zsh application cursor mode: ESC O A must navigate, not leak into the command.
smoke.emit("\x1b[?1h"); await delay(30);
const before = smoke.text().length;
key("ArrowDown", 40); await delay(40);
check("app-mode arrow navigates suggestions", smoke.text().length === before, JSON.stringify(smoke.text().slice(before)));
check("second suggestion selected", document.querySelectorAll(".terminal-suggestions button")[0]?.classList.contains("selected") === true);
key("Escape", 27); await delay(30);
smoke.emit("\x1b[?1l");

// Other full-screen apps (vim, htop) own their input: no panel.
smoke.emit("\x1b[?1049h\x1b[?1003h\x1b[?1006h"); await delay(30);
type("pi"); await delay(60);
check("no suggestions in other full-screen apps", !document.querySelector(".terminal-suggestions"));
smoke.emit("\x1b[?1003l\x1b[?1006l\x1b[?1049l"); await delay(30);

// Herdr started from the prompt: its panes' shells get suggestions.
smoke.emit("\x1b]633;D;0\x07$ "); await delay(30);
type("herdr"); key("Enter", 13); await delay(30);
smoke.emit("\x1b[?1049h\x1b[?1003h\x1b[?1006hHERDR $ "); await delay(30);
type("pi"); await delay(80);
check("suggestions inside herdr", Boolean(document.querySelector(".terminal-suggestions")));
key("ArrowDown", 40); await delay(30);
type("\x1b[<35;10;4M"); await delay(40);
check("mouse move keeps panel and selection", document.querySelector(".terminal-suggestions button.selected") !== null && Boolean(document.querySelector(".terminal-suggestions")));
type("\x1b[<0;10;4M"); await delay(40);
check("click in herdr closes panel", !document.querySelector(".terminal-suggestions"));

// OSC 52 copy from herdr reaches the system clipboard; queries are refused.
smoke.emit("\x1b]52;c;" + btoa(unescape(encodeURIComponent("xin chào"))) + "\x07");
smoke.emit("\x1b]52;c;?\x07");
await delay(60);
const clip = calls.filter((c) => c.cmd === "terminal_clipboard_write");
check("osc52 copies to clipboard", clip.length === 1 && clip[0].args.text === "xin chào", JSON.stringify(clip.map((c) => c.args)));
smoke.emit("\x1b[?1003l\x1b[?1006l\x1b[?1049l"); await delay(30);
smoke.emit("\x1b[?1049h"); await delay(30);
type("pi"); await delay(60);
check("leaving herdr ends multiplexer mode", !document.querySelector(".terminal-suggestions"));
smoke.emit("\x1b[?1049l");

// Agent attention: bell, OSC 9 and OSC 777 notify; OSC 9;4 progress is ignored.
smoke.attention.length = 0;
smoke.emit("\x07"); smoke.emit("\x1b]9;4;1;50\x07"); smoke.emit("\x1b]9;Claude is waiting\x07");
smoke.emit("\x1b]777;notify;Codex;done\x07"); await delay(60);
check("bell and notifications ask for attention", smoke.attention.length === 3, JSON.stringify(smoke.attention));

const resizeCount = smoke.resizes.length;
document.querySelector("#terminal").style.display = "none";
await delay(100);
check("hidden pane does not shrink its PTY", smoke.resizes.length === resizeCount);
document.querySelector("#terminal").style.display = "block";
await delay(100);

const bench = await smoke.benchmark(20);
out.push("BENCH " + JSON.stringify(bench));
return out.join("\n");
