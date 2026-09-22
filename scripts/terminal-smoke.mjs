// pnpm dev; launch Chrome with --remote-debugging-port=9223, then node this file.
import assert from 'node:assert/strict';
const tabs = await (await fetch('http://127.0.0.1:9223/json')).json();
const socket = new WebSocket(tabs.find(tab => tab.type === 'page').webSocketDebuggerUrl);
await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
let serial = 0;
const requests = new Map();
socket.addEventListener('message', ({ data }) => {
  const message = JSON.parse(data);
  if (message.id) {
    const request = requests.get(message.id);
    requests.delete(message.id);
    if (message.error) request.reject(message.error);
    else request.resolve(message.result);
  }
});
function call(method, params = {}) {
  const id = ++serial;
  return new Promise((resolve, reject) => {
    requests.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const result = await call('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function key(key, code, keyCode, modifiers = 0) {
  await call('Input.dispatchKeyEvent', { type: 'keyDown', key, code, windowsVirtualKeyCode: keyCode, modifiers });
  await call('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: keyCode, modifiers });
  await delay(30);
}
try {
  await call('Page.navigate', { url: 'http://localhost:1420/scripts/terminal-smoke.html' });
  let ready = false;
  for (let i = 0; i < 100; i++) {
    if (await evaluate('Boolean(window.smoke)')) { ready = true; break; }
    await delay(100);
  }
  assert(ready, 'terminal harness did not mount');
  await evaluate(`smoke.emit('\x1b]633;D;0\x07$ ')`);
  await delay(60);
  await call('Input.insertText', { text: 'herdr' });
  await key('Enter', 'Enter', 13);
  await evaluate(`smoke.emit('\x1b[?1049h\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?25lHERDR'); smoke.writes.length = 0`);
  await delay(60);
  await call('Input.insertText', { text: 'pi' });
  await key('Tab', 'Tab', 9);
  await key('ArrowUp', 'ArrowUp', 38);
  await key('Escape', 'Escape', 27);
  await key('f', 'KeyF', 70, 2);
  await key('V', 'KeyV', 86, 10);
  const input = await evaluate('smoke.text()');
  assert.equal(input, 'pi\t\x1b[A\x1b\x06\x1b[200~paste test\x1b[201~');
  const snapshot = await evaluate('smoke.snapshot()');
  assert(snapshot.data.includes('\x1b[?1006h'), 'SGR mouse mode missing');
  assert(snapshot.data.includes('\x1b[?2004h'), 'bracketed paste mode missing');
  assert(snapshot.data.includes('\x1b[?25l'), 'hidden cursor missing');
  const restored = await evaluate(`smoke.restore(${JSON.stringify(snapshot)})`);
  assert(restored.data.includes('HERDR'), 'alternate screen lost');
  assert(restored.data.includes('\x1b[?1006h'), 'SGR mouse restore failed');
  assert((await evaluate('smoke.resizes.length')) > 0, 'resumed PTY was not resized');
  await evaluate(`smoke.emit('\x1b[?1049l\x1b[?25h')`);
  const metrics = await evaluate('smoke.benchmark(20)');
  assert(metrics.peakPending <= 272 * 1024, 'output queue exceeded flow-control bound');
  console.log(JSON.stringify({ input: 'PASS', snapshot: 'PASS', resize: 'PASS', metrics }, null, 2));
} finally {
  socket.close();
}
