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
