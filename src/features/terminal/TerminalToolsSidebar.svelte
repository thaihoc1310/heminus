<script lang="ts">
  import Icon from "../../components/Icon.svelte";
  import {
    clearCommandHistory,
    deleteCommandHistory,
    deleteSnippet,
    listAllCommandHistory,
    listSnippets,
    saveSnippet
  } from "../../lib/ipc";
  import { confirmDialog } from "../../lib/dialog";
  import { terminalThemes } from "../../lib/terminalThemes";
  import {
    formatTerminalShortcut,
    setHistorySuggestions,
    setHistorySuggestionsShortcut,
    setSuggestionMinimumCharacters,
    shortcutFromKeyboardEvent,
    terminalPreferences
  } from "../../lib/terminalPreferences";
  import type { Snippet, TerminalAppearance } from "../../lib/types";

  let {
    appearance,
    historyVersion = 0,
    onappearancechange,
    onrun,
    onpaste,
    onrunall,
    onclose,
    onopenfull,
    oncatalogchange = () => {}
  }: {
    appearance: TerminalAppearance;
    historyVersion?: number;
    onappearancechange: (patch: Partial<TerminalAppearance>) => void;
    onrun: (command: string) => void;
    onpaste: (command: string) => void;
    onrunall: (command: string) => void;
    onclose: () => void;
    onopenfull: () => void;
    oncatalogchange?: (snippets: Snippet[]) => void;
  } = $props();

  type SidebarSection = "snippets" | "appearance" | "history" | "config";
  type ToolSort = "newest" | "oldest" | "az" | "za";

  const sortOptions: { value: ToolSort; label: string }[] = [
    { value: "az", label: "A–Z" },
    { value: "za", label: "Z–A" },
    { value: "newest", label: "Newest to oldest" },
    { value: "oldest", label: "Oldest to newest" }
  ];

  let section = $state<SidebarSection>("snippets");
  let snippets = $state<Snippet[]>([]);
  let history = $state<string[]>([]);
  let loadingSnippets = $state(true);
  let loadingHistory = $state(true);
  let clearingHistory = $state(false);
  let snippetQuery = $state("");
  let historyQuery = $state("");
  let snippetSort = $state<ToolSort>("az");
  let snippetSortOpen = $state(false);
  let historySort = $state<ToolSort>("newest");
  let historySortOpen = $state(false);
  let editor = $state<Snippet | null>(null);
  let savingSnippet = $state(false);
  let contextMenu = $state<{ snippet: Snippet; x: number; y: number } | null>(null);
  let historyContextMenu = $state<{ command: string; x: number; y: number } | null>(null);
  let panelElement = $state<HTMLElement>();
  let capturingHistoryShortcut = $state(false);

  const visibleSnippets = $derived.by(() => {
    const query = snippetQuery.trim().toLowerCase();
    return snippets
      .filter((snippet) =>
        `${snippet.title} ${snippet.command} ${snippet.description}`
          .toLowerCase()
          .includes(query)
      )
      .sort((left, right) => {
        if (snippetSort === "newest" || snippetSort === "oldest") {
          const order = Date.parse(right.created_at) - Date.parse(left.created_at);
          return snippetSort === "oldest" ? -order : order;
        }
        const order = left.title.localeCompare(right.title, undefined, {
          sensitivity: "base",
          numeric: true
        });
        return snippetSort === "za" ? -order : order;
      });
  });

  const visibleHistory = $derived.by(() => {
    const query = historyQuery.trim().toLowerCase();
    const commands = history.filter((command) => command.toLowerCase().includes(query));
    if (historySort === "oldest") return commands.reverse();
    if (historySort === "az" || historySort === "za") {
      commands.sort((left, right) =>
        left.localeCompare(right, undefined, {
          sensitivity: "base",
          numeric: true
        })
      );
      if (historySort === "za") commands.reverse();
    }
    return commands;
  });

  $effect(() => {
    historyVersion;
    void refreshHistory();
  });

  $effect(() => {
    void refreshSnippets();
  });

  async function refreshSnippets() {
    loadingSnippets = true;
    try {
      snippets = await listSnippets();
      oncatalogchange(snippets);
    } finally {
      loadingSnippets = false;
    }
  }

  async function refreshHistory() {
    loadingHistory = true;
    try {
      history = await listAllCommandHistory(50);
    } finally {
      loadingHistory = false;
    }
  }

  function newSnippet() {
    editor = {
      id: crypto.randomUUID(),
      title: "",
      command: "",
      description: "",
      favorite: false,
      created_at: new Date().toISOString()
    };
  }

  function editSnippet(snippet: Snippet) {
    editor = { ...snippet };
    contextMenu = null;
  }

  async function persistSnippet() {
    if (!editor || !editor.title.trim() || !editor.command.trim() || savingSnippet) return;
    savingSnippet = true;
    try {
      await saveSnippet({
        ...editor,
        title: editor.title.trim(),
        command: editor.command.trim(),
        description: editor.description.trim()
      });
      editor = null;
      await refreshSnippets();
    } finally {
      savingSnippet = false;
    }
  }

  async function removeSnippet(snippet: Snippet) {
    contextMenu = null;
    await deleteSnippet(snippet.id);
    await refreshSnippets();
  }

  function openContextMenu(event: MouseEvent, snippet: Snippet) {
    event.preventDefault();
    event.stopPropagation();
    const width = 168;
    const height = 92;
    const panelBounds = panelElement?.getBoundingClientRect();
    if (!panelBounds) return;
    historyContextMenu = null;
    contextMenu = {
      snippet,
      x: Math.max(
        8,
        Math.min(event.clientX - panelBounds.left, panelBounds.width - width - 8)
      ),
      y: Math.max(
        8,
        Math.min(event.clientY - panelBounds.top, panelBounds.height - height - 8)
      )
    };
  }

  function openHistoryContextMenu(event: MouseEvent, command: string) {
    event.preventDefault();
    event.stopPropagation();
    const width = 168;
    const height = 50;
    const panelBounds = panelElement?.getBoundingClientRect();
    if (!panelBounds) return;
    contextMenu = null;
    historyContextMenu = {
      command,
      x: Math.max(
        8,
        Math.min(event.clientX - panelBounds.left, panelBounds.width - width - 8)
      ),
      y: Math.max(
        8,
        Math.min(event.clientY - panelBounds.top, panelBounds.height - height - 8)
      )
    };
  }

  async function removeHistoryCommand(command: string) {
    historyContextMenu = null;
    await deleteCommandHistory(command);
    await refreshHistory();
  }

  async function clearHistory() {
    if (clearingHistory || history.length === 0) return;
    if (!(await confirmDialog({
      title: "Clear command history?",
      message: "Remove all saved successful commands from every terminal? This cannot be undone.",
      confirmLabel: "Clear history",
      danger: true
    }))) return;
    clearingHistory = true;
    try {
      await clearCommandHistory();
      history = [];
      historyQuery = "";
      historyContextMenu = null;
    } finally {
      clearingHistory = false;
    }
  }

  function setSection(next: SidebarSection) {
    section = next;
    contextMenu = null;
    historyContextMenu = null;
    snippetSortOpen = false;
    historySortOpen = false;
  }

  function changeFontSize(delta: number) {
    const fontSize = Math.max(9, Math.min(32, appearance.fontSize + delta));
    onappearancechange({ fontSize });
  }

  function captureHistoryShortcut(event: KeyboardEvent) {
    if (!capturingHistoryShortcut) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      capturingHistoryShortcut = false;
      return;
    }
    const shortcut = shortcutFromKeyboardEvent(event);
    if (!shortcut) return;
    setHistorySuggestionsShortcut(shortcut);
    capturingHistoryShortcut = false;
  }
</script>

<svelte:window onclick={(event) => {
  const target = event.target as Element | null;
  if (target?.closest(".terminal-snippet-context")) return;
  contextMenu = null;
  historyContextMenu = null;
  if (target?.closest(".terminal-tool-menu-anchor")) return;
  snippetSortOpen = false;
  historySortOpen = false;
}} />

<aside
  bind:this={panelElement}
  class="terminal-tools-panel"
  aria-label="Terminal tools"
>
  <header class="terminal-tools-header">
    <nav aria-label="Terminal tool sections">
      <button
        class:active={section === "snippets"}
        title="Snippets"
        onclick={() => setSection("snippets")}
      ><Icon name="code" size={18} /></button>
      <button
        class:active={section === "history"}
        title="Successful command history"
        onclick={() => setSection("history")}
      ><Icon name="clock" size={18} /></button>
      <button
        class:active={section === "appearance"}
        title="Appearance"
        onclick={() => setSection("appearance")}
      ><Icon name="sun" size={18} /></button>
      <button
        class:active={section === "config"}
        title="Terminal configuration"
        onclick={() => setSection("config")}
      ><Icon name="appearance" size={18} /></button>
    </nav>
    <button class="terminal-tools-close" title="Close terminal tools" onclick={onclose}>
      <Icon name="close" size={16} />
    </button>
  </header>

  {#if section === "snippets"}
    <div class="terminal-tools-toolbar">
      <button class="terminal-tool-primary" onclick={newSnippet}>
        <Icon name="plus" size={15} /> New snippet
      </button>
      <label class="terminal-tool-search">
        <Icon name="search" size={15} />
        <input bind:value={snippetQuery} placeholder="Search" aria-label="Search snippets" />
      </label>
      <div class="terminal-tool-menu-anchor">
        <button
          class="terminal-tool-icon"
          title="Sort snippets"
          aria-label="Sort snippets"
          aria-expanded={snippetSortOpen}
          onclick={(event) => {
            event.stopPropagation();
            historySortOpen = false;
            snippetSortOpen = !snippetSortOpen;
          }}
        ><Icon name="sort" size={16} /></button>
        {#if snippetSortOpen}
          <div class="terminal-tool-popover snippet-sort-popover">
            {#each sortOptions as option}
              {#if option.value === "newest"}<div class="terminal-sort-separator"></div>{/if}
              <button class:active={snippetSort === option.value} onclick={() => {
                snippetSort = option.value;
                snippetSortOpen = false;
              }}>{option.label} <Icon name="check" size={14} /></button>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    {#if editor}
      <div class="terminal-snippet-editor">
        <div class="terminal-snippet-editor-heading">
          <strong>{snippets.some((snippet) => snippet.id === editor?.id) ? "Edit snippet" : "New snippet"}</strong>
          <button onclick={() => (editor = null)} title="Cancel">
            <Icon name="close" size={14} />
          </button>
        </div>
        <input bind:value={editor.title} placeholder="Name" aria-label="Snippet name" />
        <textarea bind:value={editor.command} placeholder="Shell command" aria-label="Shell command"></textarea>
        <input bind:value={editor.description} placeholder="Description (optional)" aria-label="Description" />
        <button
          class="terminal-tool-primary save-snippet"
          disabled={!editor.title.trim() || !editor.command.trim() || savingSnippet}
          onclick={() => void persistSnippet()}
        >{savingSnippet ? "Saving…" : "Save snippet"}</button>
      </div>
    {/if}

    <div class="terminal-tools-scroll">
      {#if loadingSnippets}
        <p class="terminal-tools-empty">Loading snippets…</p>
      {:else if visibleSnippets.length === 0}
        <div class="terminal-tools-empty">
          <Icon name="code" size={23} />
          <strong>{snippetQuery ? "No matching snippets" : "No snippets yet"}</strong>
          <span>Create one here or in the Snippets vault page.</span>
        </div>
      {:else}
        <div class="terminal-command-list">
          {#each visibleSnippets as snippet (snippet.id)}
            <article
              class="terminal-command-card"
              oncontextmenu={(event) => openContextMenu(event, snippet)}
            >
              <div class="terminal-command-heading">
                <span><Icon name="code" size={15} /><strong>{snippet.title}</strong></span>
                <div class="terminal-command-actions">
                  <button onclick={() => onrun(snippet.command)}>Run</button>
                  <button onclick={() => onpaste(snippet.command)}>Paste</button>
                </div>
              </div>
              <code>{snippet.command}</code>
              {#if snippet.description}<small>{snippet.description}</small>{/if}
            </article>
          {/each}
        </div>
      {/if}
    </div>
  {:else if section === "appearance"}
    <div class="terminal-tools-scroll appearance-scroll">
      <section class="terminal-text-size">
        <span>Text size</span>
        <div>
          <button title="Decrease text size" onclick={() => changeFontSize(-1)}>−</button>
          <output>{appearance.fontSize}</output>
          <button title="Increase text size" onclick={() => changeFontSize(1)}>+</button>
        </div>
      </section>

      <section class="terminal-appearance-section theme-section">
        <div class="terminal-appearance-heading"><span>Themes</span></div>
        <div class="terminal-sidebar-themes">
          {#each terminalThemes as theme (theme.id)}
            <button
              class:selected={appearance.theme === theme.id}
              onclick={() => onappearancechange({ theme: theme.id })}
            >
              <span
                class="terminal-sidebar-theme-preview"
                style={`--theme-bg:${theme.preview[0]};--theme-fg:${theme.preview[1]};--theme-a:${theme.preview[2]};--theme-b:${theme.preview[3]}`}
              ><i></i><i></i><i></i><i></i></span>
              <span><strong>{theme.label}</strong><small>{theme.description}</small></span>
              {#if appearance.theme === theme.id}<Icon name="check" size={15} />{/if}
            </button>
          {/each}
        </div>
      </section>
    </div>
  {:else if section === "history"}
    <div class="terminal-tools-toolbar history-toolbar">
      <label class="terminal-tool-search">
        <Icon name="search" size={15} />
        <input bind:value={historyQuery} placeholder="Search history" aria-label="Search command history" />
      </label>
      <button
        class="terminal-tool-icon"
        title="Clear command history"
        aria-label="Clear command history"
        disabled={loadingHistory || clearingHistory || history.length === 0}
        onclick={clearHistory}
      ><Icon name="trash" size={16} /></button>
      <div class="terminal-tool-menu-anchor">
        <button
          class="terminal-tool-icon"
          title="Sort command history"
          aria-label="Sort command history"
          aria-expanded={historySortOpen}
          onclick={(event) => {
            event.stopPropagation();
            snippetSortOpen = false;
            historySortOpen = !historySortOpen;
          }}
        ><Icon name="sort" size={16} /></button>
        {#if historySortOpen}
          <div class="terminal-tool-popover snippet-sort-popover history-sort-popover">
            {#each sortOptions as option}
              {#if option.value === "newest"}<div class="terminal-sort-separator"></div>{/if}
              <button class:active={historySort === option.value} onclick={() => {
                historySort = option.value;
                historySortOpen = false;
              }}>{option.label} <Icon name="check" size={14} /></button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    <div class="terminal-tools-scroll">
      {#if loadingHistory}
        <p class="terminal-tools-empty">Loading history…</p>
      {:else if visibleHistory.length === 0}
        <div class="terminal-tools-empty">
          <Icon name="clock" size={23} />
          <strong>No successful commands yet</strong>
          <span>Failed commands are never added here.</span>
        </div>
      {:else}
        <div class="terminal-command-list history-list">
          {#each visibleHistory as command (command)}
            <article
              class="terminal-command-card history-card"
              oncontextmenu={(event) => openHistoryContextMenu(event, command)}
            >
              <code>{command}</code>
              <div class="terminal-command-actions">
                <button onclick={() => onrun(command)}>Run</button>
                <button onclick={() => onpaste(command)}>Paste</button>
              </div>
            </article>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <div class="terminal-tools-scroll terminal-config-scroll">
      <section class="terminal-config-section">
        <header>
          <strong>Suggestions</strong>
          <small>Choose what appears while you type.</small>
        </header>
        <div class="terminal-config-row">
          <span class="terminal-config-copy">
            <strong>History suggestions</strong>
            <small>Mix successful commands with snippets.</small>
          </span>
          <button
            class="terminal-shortcut-button"
            class:capturing={capturingHistoryShortcut}
            title="Change history suggestions shortcut"
            aria-label="Change history suggestions shortcut"
            onclick={() => capturingHistoryShortcut = true}
            onkeydown={captureHistoryShortcut}
            onblur={() => capturingHistoryShortcut = false}
          >{capturingHistoryShortcut
              ? "Press shortcut…"
              : formatTerminalShortcut($terminalPreferences.historySuggestionsShortcut)}</button>
          <button
            class="terminal-config-toggle"
            aria-label="Toggle history suggestions"
            aria-pressed={$terminalPreferences.historySuggestions}
            onclick={() => setHistorySuggestions(!$terminalPreferences.historySuggestions)}
          >
            <span
              class="terminal-config-switch"
              class:active={$terminalPreferences.historySuggestions}
              aria-hidden="true"
            ><i></i></span>
          </button>
        </div>
        <div class="terminal-config-row terminal-config-number-row">
          <span class="terminal-config-copy">
            <strong>Minimum characters</strong>
            <small>Start suggesting after this many typed characters.</small>
          </span>
          <div class="terminal-config-stepper" aria-label="Minimum suggestion characters">
            <button
              title="Decrease minimum characters"
              aria-label="Decrease minimum characters"
              disabled={$terminalPreferences.suggestionMinimumCharacters <= 1}
              onclick={() => setSuggestionMinimumCharacters(
                $terminalPreferences.suggestionMinimumCharacters - 1
              )}
            >−</button>
            <output aria-live="polite">{$terminalPreferences.suggestionMinimumCharacters}</output>
            <button
              title="Increase minimum characters"
              aria-label="Increase minimum characters"
              disabled={$terminalPreferences.suggestionMinimumCharacters >= 10}
              onclick={() => setSuggestionMinimumCharacters(
                $terminalPreferences.suggestionMinimumCharacters + 1
              )}
            >+</button>
          </div>
        </div>
      </section>
    </div>
  {/if}

  {#if contextMenu}
    <div
      class="terminal-snippet-context"
      role="menu"
      tabindex="-1"
      style:left={`${contextMenu.x}px`}
      style:top={`${contextMenu.y}px`}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <button onclick={() => editSnippet(contextMenu!.snippet)}>
        <Icon name="edit" size={15} /> Edit
      </button>
      <button class="danger" onclick={() => void removeSnippet(contextMenu!.snippet)}>
        <Icon name="trash" size={15} /> Delete
      </button>
    </div>
  {/if}

  {#if historyContextMenu}
    <div
      class="terminal-snippet-context terminal-history-context"
      role="menu"
      tabindex="-1"
      style:left={`${historyContextMenu.x}px`}
      style:top={`${historyContextMenu.y}px`}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <button
        class="danger"
        onclick={() => void removeHistoryCommand(historyContextMenu!.command)}
      >
        <Icon name="trash" size={15} /> Delete
      </button>
    </div>
  {/if}
</aside>
