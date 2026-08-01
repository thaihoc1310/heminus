<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { Terminal } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { onMount, tick } from "svelte";
  import Icon from "../../components/Icon.svelte";
  import HostIcon from "../../components/HostIcon.svelte";
  import {
    attachTerminal,
    closeTerminal,
    listCommandHistory,
    listSnippets,
    openTerminal,
    recordCommandHistory,
    resizeTerminal,
    writeTerminal
  } from "../../lib/ipc";
  import type {
    Host,
    Snippet,
    TerminalAppearance,
    TerminalCommandRequest,
    TerminalEvent
  } from "../../lib/types";
  import { terminalTheme } from "../../lib/terminalThemes";
  import {
    detectHostOperatingSystem,
    rememberHostOperatingSystem
  } from "../../lib/hostOperatingSystem";
  import { consumeShellCompletionMarkers } from "../../lib/terminalShellIntegration";
  import {
    buildTerminalSuggestions,
    highlightedCommand,
    updateCommandInput,
    type TerminalSuggestion
  } from "../../lib/terminalSuggestions";

  let {
    paneId,
    title,
    host = null,
    appearance,
    snippetVersion = 0,
    historyVersion = 0,
    resumeSessionId = null,
    commandRequest = null,
    workspace = false,
    broadcast = false,
    onClose = () => {},
    onBroadcast = () => {},
    onFocus = () => {},
    onHeaderPointerDown = (_event: PointerEvent) => {},
    onSessionReady = (_paneId: string, _sessionId: string) => {},
    onSessionClosed = (_paneId: string) => {},
    onUserInput = (_paneId: string, _bytes: number[]) => {},
    onActivate = (_paneId: string) => {},
    onCommandRecorded = () => {},
    shouldPreserveSession = () => false
  }: {
    paneId: string;
    title: string;
    host?: Host | null;
    appearance: TerminalAppearance;
    snippetVersion?: number;
    historyVersion?: number;
    resumeSessionId?: string | null;
    commandRequest?: TerminalCommandRequest | null;
    workspace?: boolean;
    broadcast?: boolean;
    onClose?: () => void;
    onBroadcast?: () => void;
    onFocus?: () => void;
    onHeaderPointerDown?: (event: PointerEvent) => void;
    onSessionReady?: (paneId: string, sessionId: string) => void;
    onSessionClosed?: (paneId: string) => void;
    onUserInput?: (paneId: string, bytes: number[]) => void;
    onActivate?: (paneId: string) => void;
    onCommandRecorded?: () => void;
    shouldPreserveSession?: () => boolean;
  } = $props();

  let container: HTMLDivElement;
  let error = $state("");
  let commandInput = $state("");
  let suggestions = $state<TerminalSuggestion[]>([]);
  let suggestionIndex = $state(-1);
  let suggestionStyle = $state("");
  let applySuggestion: ((command: string) => void) | null = null;
  let terminalApi: Terminal | null = null;
  let refitTerminal: (() => void) | null = null;
  let externalCommandHandler: ((command: string, run: boolean) => void) | null = null;
  let commandHandlerVersion = $state(0);
  let snippetCatalog = $state<Snippet[]>([]);
  let commandHistory: string[] = [];
  let searchOpen = $state(false);
  let searchQuery = $state("");
  let searchResultIndex = $state(-1);
  let searchResultCount = $state(0);
  let searchInput = $state<HTMLInputElement>();
  type TerminalSearchMatch = {
    row: number;
    column: number;
    length: number;
  };
  let searchMatches: TerminalSearchMatch[] = [];
  let lastCommandRequestId = "";
  let operatingSystemDetectionBuffer = "";
  let operatingSystemDetected = false;
  let activeTheme = $derived(terminalTheme(appearance.theme));
  let terminalStyle = $derived(
    [
      `--terminal-background:${activeTheme.palette.background ?? "#1e2228"}`,
      `--terminal-foreground:${activeTheme.palette.foreground ?? "#c9d1d9"}`,
      `--terminal-cursor:${activeTheme.palette.cursor ?? activeTheme.palette.foreground ?? "#c9d1d9"}`,
      `--terminal-cursor-accent:${activeTheme.palette.cursorAccent ?? activeTheme.palette.background ?? "#1e2228"}`,
      `--terminal-header-background:${activeTheme.chrome.headerBackground}`,
      `--terminal-header-foreground:${activeTheme.chrome.headerForeground}`,
      `--terminal-header-muted:${activeTheme.chrome.headerMuted}`,
      `--terminal-header-border:${activeTheme.chrome.headerBorder}`,
      `--terminal-control-hover:${activeTheme.chrome.controlHover}`,
      `--terminal-active-foreground:${activeTheme.chrome.activeForeground}`,
      `--terminal-active-background:${activeTheme.chrome.activeBackground}`,
      `--terminal-selection:${activeTheme.palette.selectionBackground ?? activeTheme.chrome.activeBackground}`
    ].join(";")
  );

  $effect(() => {
    const palette = activeTheme.palette;
    if (terminalApi) {
      terminalApi.options.theme = { ...palette };
    }
  });

  $effect(() => {
    const fontSize = appearance.fontSize;
    if (!terminalApi) return;
    terminalApi.options.fontSize = fontSize;
    window.requestAnimationFrame(() => refitTerminal?.());
  });

  $effect(() => {
    commandHandlerVersion;
    const request = commandRequest;
    if (
      !request ||
      request.id === lastCommandRequestId ||
      !externalCommandHandler
    ) return;
    lastCommandRequestId = request.id;
    externalCommandHandler(request.command, request.run);
  });

  $effect(() => {
    snippetVersion;
    let disposed = false;
    void listSnippets().then((items) => {
      if (!disposed) snippetCatalog = items;
    });
    return () => {
      disposed = true;
    };
  });

  $effect(() => {
    historyVersion;
    const hostId = host?.id ?? null;
    let disposed = false;
    void listCommandHistory(hostId).then((items) => {
      if (!disposed) commandHistory = items;
    });
    return () => {
      disposed = true;
    };
  });

  function chooseSuggestion(index: number) {
    const suggestion = suggestions[index];
    if (suggestion) applySuggestion?.(suggestion.command);
  }

  function handleHeaderPointerDown(event: PointerEvent) {
    if ((event.target as HTMLElement).closest("button")) {
      return;
    }
    onHeaderPointerDown(event);
  }

  function collectTerminalSearchMatches(query: string): TerminalSearchMatch[] {
    const terminal = terminalApi;
    if (!terminal || !query) return [];
    const needle = query.toLowerCase();
    const matches: TerminalSearchMatch[] = [];
    const buffer = terminal.buffer.active;

    for (let row = 0; row < buffer.length; row += 1) {
      const line = buffer.getLine(row);
      if (!line) continue;
      let text = "";
      const columns: number[] = [];

      for (let column = 0; column < terminal.cols; column += 1) {
        const cell = line.getCell(column);
        if (!cell || cell.getWidth() === 0) continue;
        const characters = cell.getChars() || " ";
        text += characters;
        for (let index = 0; index < characters.length; index += 1) {
          columns.push(column);
        }
      }

      const haystack = text.toLowerCase();
      let offset = 0;
      while (offset <= haystack.length - needle.length) {
        const matchOffset = haystack.indexOf(needle, offset);
        if (matchOffset < 0) break;
        const startColumn = columns[matchOffset] ?? matchOffset;
        const finalCharacter = matchOffset + needle.length - 1;
        const finalColumn = columns[finalCharacter] ?? startColumn;
        const finalCellWidth = line.getCell(finalColumn)?.getWidth() ?? 1;
        matches.push({
          row,
          column: startColumn,
          length: Math.max(1, finalColumn + finalCellWidth - startColumn)
        });
        offset = matchOffset + Math.max(1, needle.length);
      }
    }

    return matches;
  }

  function selectTerminalSearchMatch(match: TerminalSearchMatch) {
    const terminal = terminalApi;
    if (!terminal) return;
    terminal.select(match.column, match.row, match.length);
    terminal.scrollToLine(
      Math.max(0, match.row - Math.floor(terminal.rows / 2))
    );
  }

  function findInTerminal(
    direction: "next" | "previous",
    query = searchQuery
  ) {
    const queryChanged = query !== searchQuery;
    searchQuery = query;
    if (!query) {
      terminalApi?.clearSelection();
      searchMatches = [];
      searchResultIndex = -1;
      searchResultCount = 0;
      return;
    }

    if (queryChanged || searchMatches.length === 0) {
      searchMatches = collectTerminalSearchMatches(query);
      searchResultCount = searchMatches.length;
      searchResultIndex =
        searchMatches.length === 0
          ? -1
          : direction === "previous"
            ? searchMatches.length - 1
            : 0;
    } else if (direction === "previous") {
      searchResultIndex =
        (searchResultIndex - 1 + searchMatches.length) % searchMatches.length;
    } else {
      searchResultIndex = (searchResultIndex + 1) % searchMatches.length;
    }

    const match = searchMatches[searchResultIndex];
    if (match) selectTerminalSearchMatch(match);
    else terminalApi?.clearSelection();
  }

  function handleSearchInput(event: Event) {
    const query = (event.currentTarget as HTMLInputElement).value;
    findInTerminal("next", query);
  }

  function openTerminalSearch() {
    searchOpen = true;
    suggestions = [];
    suggestionIndex = -1;
    void tick().then(() => {
      searchInput?.focus();
      searchInput?.select();
    });
  }

  function closeTerminalSearch() {
    searchOpen = false;
    terminalApi?.clearSelection();
    searchMatches = [];
    searchResultIndex = -1;
    searchResultCount = 0;
    terminalApi?.focus();
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchInput?.select();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeTerminalSearch();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      findInTerminal(event.shiftKey ? "previous" : "next");
    }
  }

  onMount(() => {
    let disposed = false;
    let sessionId: string | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let attachedEventUnlisten: (() => void) | null = null;
    let resizeFrame: number | null = null;
    let flushTimer: number | null = null;
    let queuedInput: number[] = [];
    let writeChain = Promise.resolve();
    let terminal: Terminal | null = null;
    let fitAddon: FitAddon | null = null;
    let webLinksAddon: WebLinksAddon | null = null;
    let pendingCommands: string[] = [];
    let shellIntegrationRemainder = "";
    let outputTail = "";
    let sensitiveInput = false;
    const outputDecoder = new TextDecoder();
    const activate = () => onActivate(paneId);

    async function start() {
      if (disposed) return;

      terminal = new Terminal({
        allowProposedApi: false,
        convertEol: false,
        cursorBlink: true,
        fontFamily: "'JetBrains Mono', 'Ubuntu Mono', monospace",
        fontSize: appearance.fontSize,
        fontWeight: "400",
        fontWeightBold: "600",
        letterSpacing: 0.1,
        lineHeight: 1.16,
        scrollback: 20_000,
        smoothScrollDuration: 80,
        theme: terminalTheme(appearance.theme).palette
      });
      terminalApi = terminal;
      fitAddon = new FitAddon();
      webLinksAddon = new WebLinksAddon();
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(webLinksAddon);
      terminal.open(container);
      container.addEventListener("focusin", activate);
      fitAddon.fit();
      const channel = new Channel<TerminalEvent>();
      const handleTerminalEvent = (event: TerminalEvent) => {
        if (!terminal) return;
        if (event.kind === "output") {
          terminal.write(new Uint8Array(event.bytes));
          const text = outputDecoder
            .decode(new Uint8Array(event.bytes), { stream: true })
            .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "");
          if (host && !operatingSystemDetected) {
            operatingSystemDetectionBuffer = `${operatingSystemDetectionBuffer}${text}`.slice(-16_384);
            const detected = detectHostOperatingSystem(operatingSystemDetectionBuffer);
            if (detected) {
              rememberHostOperatingSystem(host.id, detected);
              operatingSystemDetected = true;
            }
          }
          const completionScan = consumeShellCompletionMarkers(
            shellIntegrationRemainder,
            text
          );
          shellIntegrationRemainder = completionScan.remainder;
          for (const exitCode of completionScan.exitCodes) {
            const command = pendingCommands.shift();
            if (!command || exitCode !== 0) continue;
            commandHistory = [
              command,
              ...commandHistory.filter((item) => item !== command)
            ].slice(0, 200);
            void recordCommandHistory(host?.id ?? null, command).then((recorded) => {
              if (recorded) onCommandRecorded();
            });
          }
          outputTail = `${outputTail}${text}`.slice(-320);
          if (
            /(?:password|passphrase|verification code|one-time code|otp|token)[^:\n]{0,40}:\s*$/i.test(
              outputTail
            )
          ) {
            sensitiveInput = true;
            commandInput = "";
            suggestions = [];
          }
        }
        if (event.kind === "error") {
          error = event.message;
        }
        if (event.kind === "exit") {
          if (sessionId) {
            onSessionClosed(paneId);
            sessionId = null;
          }
        }
        if (event.kind === "disconnect") {
          if (sessionId) {
            onSessionClosed(paneId);
            sessionId = null;
          }
          onClose();
        }
      };
      channel.onmessage = handleTerminalEvent;

      if (resumeSessionId) {
        try {
          attachedEventUnlisten = await attachTerminal(
            resumeSessionId,
            handleTerminalEvent
          );
          sessionId = resumeSessionId;
        } catch {
          sessionId = await openTerminal(terminal.rows, terminal.cols, channel, host, title);
        }
      } else {
        sessionId = await openTerminal(terminal.rows, terminal.cols, channel, host, title);
      }
      let lastPtyRows = terminal.rows;
      let lastPtyCols = terminal.cols;
      onSessionReady(paneId, sessionId);
      const encoder = new TextEncoder();
      const flush = () => {
        flushTimer = null;
        if (!sessionId || queuedInput.length === 0) return;
        const bytes = queuedInput;
        queuedInput = [];
        const id = sessionId;
        writeChain = writeChain
          .then(() => writeTerminal(id, bytes))
          .catch((cause: unknown) => {
            error = cause instanceof Error ? cause.message : String(cause);
          });
      };
      const queueData = (data: string) => {
        const bytes = [...encoder.encode(data)];
        queuedInput.push(...bytes);
        onUserInput(paneId, bytes);
        if (flushTimer === null) flushTimer = window.setTimeout(flush, 4);
      };
      externalCommandHandler = (command: string, run: boolean) => {
        const data = `${command}${run ? "\r" : ""}`;
        queueData(data);
        const update = updateCommandInput(commandInput, data);
        commandInput = update.input;
        pendingCommands.push(...update.submitted);
        suggestions = [];
        suggestionIndex = -1;
        terminal?.focus();
      };
      commandHandlerVersion += 1;
      const positionSuggestions = () => {
        if (!terminal || suggestions.length === 0) return;
        const cellWidth = container.clientWidth / Math.max(terminal.cols, 1);
        const cellHeight = container.clientHeight / Math.max(terminal.rows, 1);
        const width = Math.min(520, Math.max(290, container.clientWidth - 20));
        const height = Math.min(suggestions.length, 7) * 42 + 42;
        const cursorLeft = 10 + terminal.buffer.active.cursorX * cellWidth;
        const cursorTop = container.offsetTop + 8 + terminal.buffer.active.cursorY * cellHeight;
        const left = Math.max(10, Math.min(cursorLeft, container.clientWidth - width - 10));
        const top =
          cursorTop > height + 18
            ? cursorTop - height - 5
            : Math.min(cursorTop + cellHeight + 5, container.clientHeight - height - 10);
        suggestionStyle = `left:${left}px;top:${Math.max(container.offsetTop + 8, top)}px;width:${width}px`;
      };
      const refreshSuggestions = () => {
        suggestions = buildTerminalSuggestions(commandInput, snippetCatalog, commandHistory);
        suggestionIndex = -1;
        requestAnimationFrame(positionSuggestions);
      };
      applySuggestion = (command: string) => {
        const erase = "\x7f".repeat([...commandInput].length);
        queueData(`${erase}${command}`);
        commandInput = command;
        suggestions = [];
        suggestionIndex = -1;
        terminal?.focus();
      };
      terminal.onData((data) => {
        if (sensitiveInput) {
          queueData(data);
          if (data.includes("\r") || data.includes("\n") || data.includes("\x03")) {
            sensitiveInput = false;
            commandInput = "";
            suggestions = [];
          }
          return;
        }
        if (suggestions.length > 0) {
          if (data === "\x1b[A" || data === "\x1b[B") {
            const direction = data === "\x1b[A" ? -1 : 1;
            const start = suggestionIndex < 0 ? (direction > 0 ? -1 : 0) : suggestionIndex;
            suggestionIndex = (start + direction + suggestions.length) % suggestions.length;
            return;
          }
          if (data === "\x1b") {
            suggestions = [];
            suggestionIndex = -1;
            return;
          }
          if (data === "\t" || (data === "\r" && suggestionIndex >= 0)) {
            chooseSuggestion(suggestionIndex >= 0 ? suggestionIndex : 0);
            return;
          }
        }
        queueData(data);
        const update = updateCommandInput(commandInput, data);
        commandInput = update.input;
        for (const command of update.submitted) {
          pendingCommands.push(command);
        }
        refreshSuggestions();
      });
      terminal.attachCustomKeyEventHandler((event) => {
        if (
          event.type === "keydown" &&
          (event.ctrlKey || event.metaKey) &&
          event.key.toLowerCase() === "f"
        ) {
          event.preventDefault();
          openTerminalSearch();
          return false;
        }
        return true;
      });

      const syncTerminalSize = () => {
        if (resizeFrame !== null) return;
        resizeFrame = window.requestAnimationFrame(() => {
          resizeFrame = null;
          if (!terminal || !fitAddon) return;
          fitAddon.fit();
          positionSuggestions();
          if (
            !sessionId ||
            (terminal.rows === lastPtyRows && terminal.cols === lastPtyCols)
          ) return;
          lastPtyRows = terminal.rows;
          lastPtyCols = terminal.cols;
          void resizeTerminal(sessionId, terminal.rows, terminal.cols).catch(
            (cause: unknown) => {
              error = cause instanceof Error ? cause.message : String(cause);
            }
          );
        });
      };
      resizeObserver = new ResizeObserver(syncTerminalSize);
      resizeObserver.observe(container);
      refitTerminal = syncTerminalSize;
      terminal.focus();
    }

    void start().catch((cause: unknown) => {
      error = cause instanceof Error ? cause.message : String(cause);
    });

    return () => {
      const preserveSession = shouldPreserveSession();
      disposed = true;
      if (flushTimer !== null) window.clearTimeout(flushTimer);
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      resizeObserver?.disconnect();
      attachedEventUnlisten?.();
      attachedEventUnlisten = null;
      container?.removeEventListener("focusin", activate);
      terminal?.dispose();
      terminalApi = null;
      searchMatches = [];
      refitTerminal = null;
      externalCommandHandler = null;
      commandHandlerVersion += 1;
      applySuggestion = null;
      suggestions = [];
      onSessionClosed(paneId);
      if (sessionId && !preserveSession) void closeTerminal(sessionId);
    };
  });
</script>

<section
  class="terminal-pane"
  class:workspace-pane={workspace}
  style={terminalStyle}
  aria-label={host ? `SSH terminal for ${host.label}` : "Local terminal"}
>
  {#if workspace}
    <header
      class="workspace-pane-header"
      role="group"
      aria-label="Draggable terminal pane header"
      title="Drag to rearrange this pane"
      onpointerdown={handleHeaderPointerDown}
    >
      <div>
        <HostIcon hostId={host?.id} size={15} />
        <strong>{host?.label ?? "Local Terminal"}</strong>
        <span>{host ? `/ ${host.username}` : "~"}</span>
      </div>
      <nav aria-label="Workspace pane actions">
        <button class:active={broadcast} title="Broadcast input" onclick={onBroadcast}>
          <Icon name="connect" size={15} />
        </button>
        <button title="Focus pane" onclick={onFocus}>
          <Icon name="maximize" size={15} />
        </button>
        <button title="Close pane" onclick={onClose}>
          <Icon name="close" size={15} />
        </button>
      </nav>
    </header>
  {/if}
  {#if error}<div class="terminal-error">{error}</div>{/if}
  {#if searchOpen}
    <div
      class="terminal-search"
      role="search"
      onpointerdown={(event) => event.stopPropagation()}
    >
      <Icon name="search" size={15} />
      <input
        bind:this={searchInput}
        value={searchQuery}
        aria-label="Find in terminal"
        placeholder="Find"
        spellcheck="false"
        oninput={handleSearchInput}
        onkeydown={handleSearchKeydown}
      />
      <span class="terminal-search-count">
        {searchResultCount > 0 ? searchResultIndex + 1 : 0}/{searchResultCount}
      </span>
      <button
        class="terminal-search-previous"
        title="Previous match (Shift+Enter)"
        disabled={!searchQuery}
        onclick={() => findInTerminal("previous")}
      ><Icon name="chevron" size={15} /></button>
      <button
        class="terminal-search-next"
        title="Next match (Enter)"
        disabled={!searchQuery}
        onclick={() => findInTerminal("next")}
      ><Icon name="chevron" size={15} /></button>
      <button title="Close (Escape)" onclick={closeTerminalSearch}>
        <Icon name="close" size={14} />
      </button>
    </div>
  {/if}
  <div class="terminal-mount" bind:this={container}></div>
  {#if suggestions.length > 0}
    <div
      class="terminal-suggestions"
      style={suggestionStyle}
      role="listbox"
      tabindex="-1"
      aria-label="Command suggestions"
      onpointerdown={(event) => event.preventDefault()}
    >
      <div class="terminal-suggestion-list">
        {#each suggestions as suggestion, index (`${suggestion.kind}:${suggestion.command}:${index}`)}
          <button
            class:selected={suggestionIndex === index || (suggestionIndex < 0 && index === 0)}
            role="option"
            aria-selected={suggestionIndex === index || (suggestionIndex < 0 && index === 0)}
            onclick={() => chooseSuggestion(index)}
          >
            <span class="suggestion-kind">
              <Icon name={suggestion.kind === "snippet" ? "code" : "clock"} size={17} />
            </span>
            <span class="suggestion-copy">
              <strong>
                {#each highlightedCommand(suggestion.command, commandInput) as part}
                  {#if part.match}<mark>{part.text}</mark>{:else}{part.text}{/if}
                {/each}
              </strong>
              <small>{suggestion.kind === "snippet" ? `Snippet · ${suggestion.detail}` : suggestion.detail}</small>
            </span>
          </button>
        {/each}
      </div>
    </div>
  {/if}
</section>
