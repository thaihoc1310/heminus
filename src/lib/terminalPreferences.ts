import { writable } from "svelte/store";

export interface TerminalPreferences {
  historySuggestions: boolean;
}

const storageKey = "heminus-terminal-preferences";
const defaults: TerminalPreferences = {
  historySuggestions: true
};

function load(): TerminalPreferences {
  if (typeof localStorage === "undefined") return defaults;
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? "{}") as Partial<TerminalPreferences>;
    return {
      historySuggestions: saved.historySuggestions ?? defaults.historySuggestions
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
