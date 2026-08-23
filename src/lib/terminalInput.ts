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
