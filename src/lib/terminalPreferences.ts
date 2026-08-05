import { writable } from "svelte/store";

export interface TerminalPreferences {
  historySuggestions: boolean;
  historySuggestionsShortcut: string;
}

const storageKey = "heminus-terminal-preferences";
const defaults: TerminalPreferences = {
  historySuggestions: true,
  historySuggestionsShortcut: "Ctrl+Shift+H"
};

type ShortcutEvent = Pick<
  KeyboardEvent,
  "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey"
>;

function normalizedShortcutKey(key: string): string | null {
  const normalized = key.length === 1 ? key.toUpperCase() : key;
  if (["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(normalized)) return null;
  if (normalized === " ") return "Space";
  return normalized;
}

function load(): TerminalPreferences {
  if (typeof localStorage === "undefined") return defaults;
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? "{}") as Partial<TerminalPreferences>;
    return {
      historySuggestions: saved.historySuggestions ?? defaults.historySuggestions,
      historySuggestionsShortcut:
        saved.historySuggestionsShortcut ?? defaults.historySuggestionsShortcut
    };
  } catch {
    return defaults;
  }
}

export const terminalPreferences = writable<TerminalPreferences>(load());

export function setHistorySuggestions(enabled: boolean) {
  terminalPreferences.update((current) => {
    if (current.historySuggestions === enabled) return current;
    const next = { ...current, historySuggestions: enabled };
    localStorage.setItem(storageKey, JSON.stringify(next));
    return next;
  });
}

export function setHistorySuggestionsShortcut(shortcut: string) {
  terminalPreferences.update((current) => {
    if (current.historySuggestionsShortcut === shortcut) return current;
    const next = { ...current, historySuggestionsShortcut: shortcut };
    localStorage.setItem(storageKey, JSON.stringify(next));
    return next;
  });
}

export function shortcutFromKeyboardEvent(event: ShortcutEvent): string | null {
  const key = normalizedShortcutKey(event.key);
  if (!key || (!event.ctrlKey && !event.altKey && !event.metaKey)) return null;
  return [
    event.ctrlKey ? "Ctrl" : "",
    event.altKey ? "Alt" : "",
    event.shiftKey ? "Shift" : "",
    event.metaKey ? "Meta" : "",
    key
  ].filter(Boolean).join("+");
}

export function matchesTerminalShortcut(event: ShortcutEvent, shortcut: string): boolean {
  return shortcutFromKeyboardEvent(event) === shortcut;
}

export function formatTerminalShortcut(shortcut: string): string {
  return shortcut.replaceAll("+", " ");
}

export function toggleHistorySuggestions() {
  terminalPreferences.update((current) => {
    const next = { ...current, historySuggestions: !current.historySuggestions };
    localStorage.setItem(storageKey, JSON.stringify(next));
    return next;
  });
}

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    if (event.key === storageKey) terminalPreferences.set(load());
  });
}
