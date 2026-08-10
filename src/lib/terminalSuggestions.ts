import type { Snippet } from "./types";

export interface TerminalSuggestion {
  kind: "snippet" | "history";
  command: string;
  detail: string;
}

export function buildTerminalSuggestions(
  input: string,
  snippets: Snippet[],
  history: string[],
  limit = 8,
  minimumCharacters = 2
): TerminalSuggestion[] {
  const query = input.trim().toLocaleLowerCase();
  const typedCharacterCount = Array.from(query.replace(/\s+/g, "")).length;
  if (!query || typedCharacterCount < minimumCharacters) return [];
  const matches = (value: string) => {
    const normalized = value.toLocaleLowerCase();
    return query.split(/\s+/).every((part) => normalized.includes(part));
  };
  const score = (value: string) => {
    const normalized = value.toLocaleLowerCase();
    if (normalized === query) return 0;
    if (normalized.startsWith(query)) return 1;
    if (normalized.split(/\s+/).some((part) => part.startsWith(query))) return 2;
    return 3;
  };

  const snippetMatches = snippets
    .filter((snippet) => matches(snippet.command) || matches(snippet.title))
    .sort(
      (left, right) =>
        Number(right.favorite) - Number(left.favorite) ||
        score(left.command) - score(right.command) ||
        left.title.localeCompare(right.title)
    )
    .map((snippet) => ({
      kind: "snippet" as const,
      command: snippet.command.trim(),
      detail: snippet.title
    }));
  const historyMatches = history
    .filter(matches)
    .map((command) => ({ kind: "history" as const, command, detail: "History" }));
  return [...snippetMatches, ...historyMatches].slice(0, limit);
}

export function highlightedCommand(
  command: string,
  input: string
): Array<{ text: string; match: boolean }> {
  const query = input.trim();
  if (!query) return [{ text: command, match: false }];
  const index = command.toLocaleLowerCase().indexOf(query.toLocaleLowerCase());
  if (index < 0) return [{ text: command, match: false }];
  return [
    { text: command.slice(0, index), match: false },
    { text: command.slice(index, index + query.length), match: true },
    { text: command.slice(index + query.length), match: false }
  ].filter((part) => part.text);
}

export function updateCommandInput(
  current: string,
  data: string
): { input: string; submitted: string[] } {
  const submitted: string[] = [];
  const sanitized = data.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "");
  for (const character of sanitized) {
    if (character === "\r" || character === "\n") {
      const command = current.trim();
      if (command) submitted.push(command);
      current = "";
    } else if (character === "\x7f" || character === "\b") {
      current = [...current].slice(0, -1).join("");
    } else if (character === "\x15" || character === "\x03") {
      current = "";
    } else if (character >= " ") {
      current += character;
    }
  }
  return { input: current, submitted };
}

export function reconcileRenderedCommandInput(
  trackedInput: string,
  renderedInput: string | null
): string {
  if (!renderedInput || renderedInput.length < trackedInput.length) return trackedInput;
  return renderedInput;
}
