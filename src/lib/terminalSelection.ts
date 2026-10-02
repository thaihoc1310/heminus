/**
 * Full-screen programs (Claude Code, vim, htop…) paint whole rows, padding
 * them with real spaces that xterm keeps in the selection; it only trims
 * cells that were never written. Drop that padding at each line end.
 * Soft-wrapped rows are already joined by xterm, so only real line breaks
 * are touched.
 */
export function trimSelectionLineEnds(text: string): string {
  return text.replace(/[ \t]+(?=\r?$)/gm, "");
}

/**
 * Tidies the selection HTML from xterm's serialize addon for the clipboard.
 *
 * - The addon writes `style='… font-family: <fontFamily>; …'`, and a font
 *   list quoted with single quotes ("'JetBrains Mono', monospace") ends that
 *   attribute early, losing the whole block's style. Requote it.
 * - Each row is `<div><span>…</span>…</div>`; drop the same line-end padding
 *   the plain text loses, so both flavours paste the same characters.
 * - Declare UTF-8, which some readers of text/html otherwise do not assume.
 */
export function tidySelectionHtml(html: string, fontFamily: string): string {
  // Only the declaration itself: selected text can spell out the same font list.
  const declaration = `font-family: ${fontFamily};`;
  const requoted = html.replace(declaration, `font-family: ${fontFamily.replaceAll("'", '"')};`);
  const document = new DOMParser().parseFromString(requoted, "text/html");
  const rows = document.querySelectorAll("pre > div > div");
  for (const row of rows) {
    const spans = [...row.querySelectorAll("span")];
    for (let index = spans.length - 1; index >= 0; index -= 1) {
      const span = spans[index];
      const trimmed = (span.textContent ?? "").replace(/[ \t]+$/, "");
      if (trimmed) {
        span.textContent = trimmed;
        break;
      }
      span.remove();
    }
  }
  return `<html><head><meta charset="utf-8"></head><body>${document.body.innerHTML}</body></html>`;
}
