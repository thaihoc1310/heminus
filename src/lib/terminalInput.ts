const bracketedPasteStart = "\x1b[200~";

export function normalizeLinuxTerminalInput(data: string): string {
  if (!data.includes("\u00a0") || data.includes(bracketedPasteStart)) return data;
  return data.replaceAll("\u00a0", " ");
}

export function resetLinuxTerminalComposition(
  textarea: Pick<HTMLTextAreaElement, "value">
): void {
  textarea.value = "";
}

/**
 * Collects pending terminal input as whole chunks.
 *
 * Input used to be accumulated with `queue.push(...bytes)`, which passes every
 * byte as a call argument: pasting more than roughly 128 KB overflowed the
 * stack and threw out of xterm's `onData` handler, losing the paste. Keeping
 * the chunks separate and joining them once at flush time has no such limit.
 */
export function drainTerminalInput(chunks: Uint8Array[]): number[] {
  let total = 0;
  for (const chunk of chunks) total += chunk.length;
  const bytes = new Array<number>(total);
  let offset = 0;
  for (const chunk of chunks) {
    for (let index = 0; index < chunk.length; index += 1) {
      bytes[offset + index] = chunk[index];
    }
    offset += chunk.length;
  }
  return bytes;
}
