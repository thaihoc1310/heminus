<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
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
    matchesTerminalShortcut,
    terminalPreferences,
    toggleHistorySuggestions
  } from "../../lib/terminalPreferences";
  import {
    detectHostOperatingSystem,
    rememberHostOperatingSystem
  } from "../../lib/hostOperatingSystem";
  import { consumeShellCompletionMarkers } from "../../lib/terminalShellIntegration";
  import {
    buildTerminalSuggestions,
    highlightedCommand,
    reconcileRenderedCommandInput,
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
  let searchAddonApi: SearchAddon | null = null;
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
    const historySuggestions = $terminalPreferences.historySuggestions;
    if (!historySuggestions) {
      commandHistory = [];
      return;
    }
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

  function findInTerminal(
    direction: "next" | "previous",
    query = searchQuery
  ) {
    searchQuery = query;
    const addon = searchAddonApi;
    if (!query || !addon) {
      addon?.clearDecorations();
      searchResultIndex = -1;
      searchResultCount = 0;
      return;
    }
    const options = {
      incremental: true,
      decorations: {
        matchBackground: "#465363",
        matchOverviewRuler: "#718096",
        activeMatchBackground: "#278fe8",
        activeMatchColorOverviewRuler: "#278fe8"
      }
    };
    if (direction === "previous") addon.findPrevious(query, options);
    else addon.findNext(query, options);
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
    searchAddonApi?.clearDecorations();
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
    let searchAddon: SearchAddon | null = null;
    let pendingCommands: string[] = [];
    let shellIntegrationRemainder = "";
    let outputTail = "";
    let sensitiveInput = false;
    let commandStartPosition: { row: number; column: number } | null = null;
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
      searchAddon = new SearchAddon({ highlightLimit: 2_000 });
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(webLinksAddon);
      terminal.loadAddon(searchAddon);
      searchAddonApi = searchAddon;
      searchAddon.onDidChangeResults((result) => {
        if (disposed) return;
        searchResultIndex = result.resultIndex;
        searchResultCount = result.resultCount;
      });
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
          if (disposed) {
            attachedEventUnlisten();
            attachedEventUnlisten = null;
            return;
          }
          sessionId = resumeSessionId;
        } catch {
          if (disposed) return;
          const openedSessionId = await openTerminal(terminal.rows, terminal.cols, channel, host, title);
          if (disposed) {
            void closeTerminal(openedSessionId);
            return;
          }
          sessionId = openedSessionId;
        }
      } else {
        const openedSessionId = await openTerminal(terminal.rows, terminal.cols, channel, host, title);
        if (disposed) {
          void closeTerminal(openedSessionId);
          return;
        }
        sessionId = openedSessionId;
      }
      let lastPtyRows = terminal.rows;
      let lastPtyCols = terminal.cols;
      onSessionReady(paneId, sessionId);
      const encoder = new TextEncoder();
      const cursorPosition = () => terminal
        ? {
            row: terminal.buffer.active.baseY + terminal.buffer.active.cursorY,
            column: terminal.buffer.active.cursorX
          }
        : null;
      const renderedCommand = (): string | null => {
        if (!terminal || !commandStartPosition) return null;
        const buffer = terminal.buffer.active;
        const endRow = buffer.baseY + buffer.cursorY;
        if (endRow < commandStartPosition.row) return null;
        const parts: string[] = [];
        for (let row = commandStartPosition.row; row <= endRow; row += 1) {
          const line = buffer.getLine(row);
          if (!line) return null;
          const from = row === commandStartPosition.row ? commandStartPosition.column : 0;
          const to = row === endRow ? buffer.cursorX : terminal.cols;
          parts.push(line.translateToString(false, from, to));
        }
        return parts.join("").trimEnd();
      };
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
        commandStartPosition ??= cursorPosition();
        const data = `${command}${run ? "\r" : ""}`;
        queueData(data);
        const update = updateCommandInput(commandInput, data);
        commandInput = update.input;
        pendingCommands.push(...update.submitted);
        if (update.submitted.length > 0) commandStartPosition = null;
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
        suggestions = buildTerminalSuggestions(
          commandInput,
          snippetCatalog,
          $terminalPreferences.historySuggestions ? commandHistory : []
        );
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
        const submitting = data.includes("\r") || data.includes("\n");
        if (!submitting) commandStartPosition ??= cursorPosition();
        if (submitting) {
          const rendered = renderedCommand();
          commandInput = reconcileRenderedCommandInput(commandInput, rendered);
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
        if (update.submitted.length > 0 || data.includes("\x03") || data.includes("\x15")) {
          commandStartPosition = null;
        }
        refreshSuggestions();
      });
      terminal.attachCustomKeyEventHandler((event) => {
        if (
          event.type === "keydown" &&
          matchesTerminalShortcut(event, $terminalPreferences.historySuggestionsShortcut)
        ) {
          event.preventDefault();
          event.stopPropagation();
          toggleHistorySuggestions();
          refreshSuggestions();
          return false;
        }
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
      if (!disposed) error = cause instanceof Error ? cause.message : String(cause);
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
      const mountedTerminal = terminal;
      terminal = null;
      mountedTerminal?.dispose();
      terminalApi = null;
      searchAddonApi = null;
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
