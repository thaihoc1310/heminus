<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { SerializeAddon } from "@xterm/addon-serialize";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { WebglAddon } from "@xterm/addon-webgl";
  import { Terminal } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { onMount, tick, untrack } from "svelte";
  import Icon from "../../components/Icon.svelte";
  import HostIcon from "../../components/HostIcon.svelte";
  import {
    attachTerminal,
    acknowledgeTerminal,
    pauseTerminal,
    closeTerminal,
    detachTerminal,
    listCommandHistory,
    listSnippets,
    openTerminal,
    readTerminalClipboard,
    recordCommandHistory,
    resizeTerminal,
    writeTerminalClipboard,
    writeTerminal
  } from "../../lib/ipc";
  import type {
    ConnectionLogEntry,
    ConnectionStage,
    Host,
    Snippet,
    TerminalAppearance,
    TerminalCommandRequest,
    TerminalChannelMessage,
    TerminalControlEvent,
    TerminalSnapshot
  } from "../../lib/types";
  import { decodeTerminalMessage } from "../../lib/terminalChannel";
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
  import {
    isMousePress,
    isMouseReport,
    isSecretPrompt,
    ShellPromptState,
    startsMultiplexer
  } from "../../lib/terminalShellIntegration";
  import { pendingDetaches, terminalTransfers } from "../../lib/terminalTransfer";
  import {
    drainTerminalInput,
    normalizeLinuxTerminalInput,
    resetLinuxTerminalComposition
  } from "../../lib/terminalInput";
  import {
    nextTerminalFontSize,
    terminalZoomActionFromKeyboard,
    terminalZoomActionFromWheel,
    type TerminalZoomAction
  } from "../../lib/terminalZoom";
  import {
    TerminalMouseTracking,
    encodeWheelReports,
    wheelReportPosition
  } from "../../lib/terminalWheel";
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
    snapshot = undefined,
    initialCwd = null,
    commandRequest = null,
    workspace = false,
    broadcast = false,
    onClose = () => {},
    onRetry = () => {},
    onBroadcast = () => {},
    onFocus = () => {},
    onFocusModeToggle = () => {},
    headerDraggable = false,
    onHeaderPointerDown = (_event: PointerEvent) => {},
    onHeaderDragStart = (_event: DragEvent) => {},
    onHeaderDrag = (_event: DragEvent) => {},
    onHeaderDragEnd = (_event: DragEvent) => {},
    onSessionReady = (_paneId: string, _sessionId: string) => {},
    onSessionClosed = (_paneId: string, _sessionId: string | null) => {},
    onUserInput = (_paneId: string, _bytes: Uint8Array) => {},
    onActivate = (_paneId: string) => {},
    onAttention = (_paneId: string) => {},
    onCommandRecorded = () => {},
    shouldPreserveSession = () => false,
    onappearancechange = (_patch: Partial<TerminalAppearance>) => {}
  }: {
    paneId: string;
    title: string;
    host?: Host | null;
    appearance: TerminalAppearance;
    snippetVersion?: number;
    historyVersion?: number;
    resumeSessionId?: string | null;
    snapshot?: TerminalSnapshot;
    initialCwd?: string | null;
    commandRequest?: TerminalCommandRequest | null;
    workspace?: boolean;
    broadcast?: boolean;
    onClose?: () => void;
    onRetry?: () => void;
    onBroadcast?: () => void;
    onFocus?: () => void;
    onFocusModeToggle?: () => void;
    headerDraggable?: boolean;
    onHeaderPointerDown?: (event: PointerEvent) => void;
    onHeaderDragStart?: (event: DragEvent) => void;
    onHeaderDrag?: (event: DragEvent) => void;
    onHeaderDragEnd?: (event: DragEvent) => void;
    onSessionReady?: (paneId: string, sessionId: string) => void;
    onSessionClosed?: (paneId: string, sessionId: string | null) => void;
    onUserInput?: (paneId: string, bytes: Uint8Array) => void;
    onActivate?: (paneId: string) => void;
    onAttention?: (paneId: string) => void;
    onCommandRecorded?: () => void;
    shouldPreserveSession?: () => boolean;
    onappearancechange?: (patch: Partial<TerminalAppearance>) => void;
  } = $props();

  let container: HTMLDivElement;
  let error = $state("");
  const loopbackAddresses = ["127.0.0.1", "localhost", "::1"];
  // A pane keeps the same host and session for its whole lifetime, so these are
  // deliberately read once rather than tracked.
  const remoteSession = untrack(
    () => Boolean(host) && !loopbackAddresses.includes((host?.address ?? "").trim().toLowerCase())
  );
  /**
   * Remote sessions open behind a progress screen. OpenSSH writes each hop's
   * handshake to a private log file (`-E`) instead of the terminal, so this
   * overlay is the only place a refused connection, a rejected password, or a
   * jump host that never answered becomes visible.
   */
  let connectionState = $state<"connecting" | "ready" | "failed">(
    untrack(() => (remoteSession && !resumeSessionId ? "connecting" : "ready"))
  );
  /** Jump hosts in order, then the host itself. */
  let hops = $state<string[]>(untrack(() => (host ? [host.label] : [])));
  let hopStages = $state<ConnectionStage[]>(["connecting"]);
  let connectionLog = $state<ConnectionLogEntry[]>([]);
  let connectionError = $state("");
  let showConnectionLog = $state(false);
  let logScroller = $state<HTMLDivElement>();
  const connectionSteps: Array<{ stage: ConnectionStage; label: string }> = [
    { stage: "connecting", label: "Reaching the server" },
    { stage: "handshake", label: "Negotiating the SSH transport" },
    { stage: "authenticating", label: "Authenticating" },
    { stage: "authenticated", label: "Opening the shell" },
    { stage: "ready", label: "Connected" }
  ];

  function stageIndex(stage: ConnectionStage): number {
    const index = connectionSteps.findIndex((step) => step.stage === stage);
    return index < 0 ? 0 : index;
  }

  /** 0…1 along the leg that reaches this hop. */
  function hopProgress(index: number): number {
    if (connectionState === "ready") return 1;
    const stage = hopStages[index] ?? "connecting";
    if (stage === "failed") return 1;
    return stageIndex(stage) / (connectionSteps.length - 1);
  }

  function hopReached(index: number): boolean {
    return connectionState === "ready" || hopStages[index] === "ready";
  }

  function hopFailed(index: number): boolean {
    return hopStages[index] === "failed";
  }

  /** The hop the person is waiting on: the first one not yet through. */
  const activeHop = $derived(
    Math.min(
      Math.max(
        0,
        hopStages.findIndex((stage) => stage !== "ready") < 0
          ? hops.length - 1
          : hopStages.findIndex((stage) => stage !== "ready")
      ),
      Math.max(0, hops.length - 1)
    )
  );
  const connectionHeadline = $derived(
    connectionState === "failed"
      ? "Could not connect"
      : (connectionSteps[stageIndex(hopStages[activeHop] ?? "connecting")]?.label ??
        "Connecting…")
  );
  const connectionSubhead = $derived(
    hops.length > 1 && connectionState !== "ready"
      ? `${hopFailed(activeHop) || connectionState === "failed" ? "at" : "via"} ${hops[activeHop] ?? ""}`
      : ""
  );

  function setHops(labels: string[]) {
    if (labels.length === 0) return;
    hops = labels;
    hopStages = labels.map(() => "connecting");
  }

  function recordConnectionLog(entry: ConnectionLogEntry) {
    connectionLog = [...connectionLog, entry].slice(-500);
    if (connectionState !== "connecting") {
      // The handshake screen is gone, but `-E` also keeps later failures out of
      // the terminal, so a dropped session still has to say why.
      if (entry.level === "error") error = entry.message;
      return;
    }
    if (entry.stage === "failed") {
      connectionState = "failed";
      hopStages = hopStages.map((stage, index) =>
        index === entry.hop ? "failed" : stage
      );
      if (!connectionError) {
        connectionError =
          hops.length > 1 && hops[entry.hop]
            ? `${hops[entry.hop]}: ${entry.message}`
            : entry.message;
      }
      showConnectionLog = true;
    } else if (entry.stage) {
      const stage = entry.stage;
      hopStages = hopStages.map((current, index) =>
        // Stages only move forward, so a late line cannot rewind a hop.
        index === entry.hop && stageIndex(stage) > stageIndex(current) ? stage : current
      );
      if (hopStages.length > 0 && hopStages.every((current) => current === "ready")) {
        connectionState = "ready";
      }
    }
    if (showConnectionLog) {
      void tick().then(() => {
        if (logScroller) logScroller.scrollTop = logScroller.scrollHeight;
      });
    }
  }

  function markConnected() {
    connectionState = "ready";
    hopStages = hopStages.map(() => "ready");
  }

  const connectionLogIcons: Record<ConnectionLogEntry["level"], string> = {
    debug: "chevron-right",
    info: "connect",
    warning: "shield",
    error: "stop"
  };

  function connectionLogText(): string {
    return connectionLog
      .map((entry) => `[${hops[entry.hop] ?? entry.hop}] [${entry.level}] ${entry.message}`)
      .join("\n");
  }
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
  let searchSettingsOpen = $state(false);
  let searchQuery = $state("");
  let searchResultIndex = $state(-1);
  let searchResultCount = $state(0);
  let searchCaseSensitive = $state(false);
  let searchWholeWord = $state(false);
  let searchRegex = $state(false);
  let searchWrapAround = $state(true);
  let searchError = $state("");
  let searchInput = $state<HTMLInputElement>();
  let searchAddonApi: SearchAddon | null = null;
  let lastCommandRequestId = "";
  let operatingSystemDetectionBuffer = "";
  let operatingSystemDetected = false;
  let zoomHint = $state<number | null>(null);
  let zoomHintTimer: number | null = null;
  let lastWheelZoomAt = 0;

  function showZoomHint(size: number) {
    zoomHint = size;
    if (zoomHintTimer !== null) window.clearTimeout(zoomHintTimer);
    zoomHintTimer = window.setTimeout(() => {
      zoomHint = null;
      zoomHintTimer = null;
    }, 900);
  }

  function applyTerminalZoom(action: TerminalZoomAction) {
    const fontSize = nextTerminalFontSize(appearance.fontSize, action);
    if (fontSize !== appearance.fontSize) onappearancechange({ fontSize });
    showZoomHint(fontSize);
    onActivate(paneId);
  }
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
      `--terminal-selection:${activeTheme.palette.selectionBackground ?? activeTheme.chrome.activeBackground}`,
      `--terminal-scrollbar:${activeTheme.palette.scrollbarSliderBackground ?? activeTheme.chrome.headerBorder}`,
      `--terminal-scrollbar-hover:${activeTheme.palette.scrollbarSliderHoverBackground ?? activeTheme.chrome.headerMuted}`,
      `--terminal-scrollbar-active:${activeTheme.palette.scrollbarSliderActiveBackground ?? activeTheme.chrome.activeBackground}`
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
    query = searchQuery,
    force = false
  ) {
    const queryChanged = query !== searchQuery;
    searchQuery = query;
    const addon = searchAddonApi;
    if (!query || !addon) {
      addon?.clearDecorations();
      searchResultIndex = -1;
      searchResultCount = 0;
      searchError = "";
      return;
    }
    if (!force && !queryChanged && !searchWrapAround && searchResultCount > 0) {
      const reachedStart = direction === "previous" && searchResultIndex <= 0;
      const reachedEnd = direction === "next" && searchResultIndex >= searchResultCount - 1;
      if (reachedStart || reachedEnd) return;
    }
    const options = {
      caseSensitive: searchCaseSensitive,
      wholeWord: searchWholeWord,
      regex: searchRegex,
      incremental: true,
      decorations: {
        matchBackground: "#465363",
        matchOverviewRuler: "#718096",
        activeMatchBackground: "#278fe8",
        activeMatchColorOverviewRuler: "#278fe8"
      }
    };
    try {
      if (direction === "previous") addon.findPrevious(query, options);
      else addon.findNext(query, options);
      searchError = "";
    } catch {
      addon.clearDecorations();
      searchResultIndex = -1;
      searchResultCount = 0;
      searchError = "Invalid regular expression";
    }
  }

  function refreshTerminalSearch() {
    searchAddonApi?.clearDecorations();
    searchResultIndex = -1;
    searchResultCount = 0;
    findInTerminal("next", searchQuery, true);
    searchInput?.focus();
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
    searchSettingsOpen = false;
    searchAddonApi?.clearDecorations();
    searchResultIndex = -1;
    searchResultCount = 0;
    searchError = "";
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
    let resizeFrame: number | null = null;
    let primarySelectionFrame: number | null = null;
    let primarySelectionWrite = Promise.resolve();
    let flushTimer: number | null = null;
    let queuedInput: Uint8Array[] = [];
    let writeChain = Promise.resolve();
    let terminal: Terminal | null = null;
    let fitAddon: FitAddon | null = null;
    let webLinksAddon: WebLinksAddon | null = null;
    let searchAddon: SearchAddon | null = null;
    const shell = new ShellPromptState();
    // Herdr/tmux run their panes' shells inside one alternate screen, so the
    // prompt markers never reach us; remember that one was started instead.
    let insideMultiplexer = false;
    let streamId: string | null = null;
    let ackToken: string | null = null;
    let acknowledgedBytes = 0;
    let ackTimer: number | null = null;
    let checkpoint: ((snapshot: TerminalSnapshot) => void) | null = null;
    let commandStartPosition: { row: number; column: number } | null = null;
    let handleTerminalMiddleMouseEvent: ((event: MouseEvent) => void) | null = null;
    let handleTerminalMiddlePointerDown: ((event: PointerEvent) => void) | null = null;
    let handleTerminalMiddlePointerUp: ((event: PointerEvent) => void) | null = null;
    let handleTerminalAuxClick: ((event: MouseEvent) => void) | null = null;
    let handleTerminalCompositionStart: (() => void) | null = null;
    const outputDecoder = new TextDecoder();
    const activate = () => onActivate(paneId);
    const preventContextMenu = (event: MouseEvent) => event.preventDefault();
    let handleApplicationWheel: ((event: WheelEvent) => void) | null = null;
    const handleZoomWheel = (event: WheelEvent) => {
      const zoom = terminalZoomActionFromWheel(event);
      if (!zoom) return;
      event.preventDefault();
      event.stopPropagation();
      const now = performance.now();
      if (now - lastWheelZoomAt < 70) return;
      lastWheelZoomAt = now;
      applyTerminalZoom(zoom);
    };
    const supportsPrimarySelection = navigator.userAgent.includes("Linux");

    async function start() {
      if (disposed) return;

      terminal = new Terminal({
        // SearchAddon uses xterm's decoration API for highlights and result counts.
        allowProposedApi: true,
        convertEol: false,
        cursorBlink: true,
        fontFamily: "'JetBrains Mono', 'Ubuntu Mono', monospace",
        fontSize: appearance.fontSize,
        fontWeight: "400",
        fontWeightBold: "600",
        letterSpacing: 0.1,
        lineHeight: 1.16,
        scrollback: 5_000,
        smoothScrollDuration: 80,
        theme: terminalTheme(appearance.theme).palette
      });
      terminalApi = terminal;
      fitAddon = new FitAddon();
      webLinksAddon = new WebLinksAddon();
      searchAddon = new SearchAddon({ highlightLimit: 2_000 });
      const serializeAddon = new SerializeAddon();
      // SerializeAddon omits mouse encoding and cursor visibility. Herdr uses
      // SGR mouse coordinates; restoring tracking alone would corrupt clicks.
      const extraModes = new Map<number, boolean>([[25, true]]);
      const mouseTracking = new TerminalMouseTracking();
      for (const final of ["h", "l"]) {
        terminal.parser.registerCsiHandler({ prefix: "?", final }, (params) => {
          const enabled = final === "h";
          for (const mode of params) {
            if (typeof mode !== "number") continue;
            if ([25, 1005, 1006, 1015, 1016].includes(mode)) extraModes.set(mode, enabled);
            mouseTracking.setPrivateMode(mode, enabled);
          }
          return false;
        });
      }
      terminal.parser.registerEscHandler({ final: "c" }, () => {
        mouseTracking.reset();
        return false;
      });
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(webLinksAddon);
      terminal.loadAddon(searchAddon);
      terminal.loadAddon(serializeAddon);
      // Agent TUIs (Claude Code, Codex) lay out emoji and symbols with modern
      // wcwidth; Unicode 6 widths shift everything after them by a cell.
      terminal.loadAddon(new Unicode11Addon());
      terminal.unicode.activeVersion = "11";
      for (const osc of [133, 633]) {
        terminal.parser.registerOscHandler(osc, (data) => {
          if (terminal?.buffer.active.type !== "normal") return false;
          const command = shell.marker(data);
          if (command) {
            commandHistory = [command, ...commandHistory.filter((item) => item !== command)].slice(0, 200);
            void recordCommandHistory(host?.id ?? null, command).then((recorded) => {
              if (recorded) onCommandRecorded();
            });
          }
          return true;
        });
      }
      // OSC 52: Herdr and tmux copy mode set the system clipboard this way.
      // Clipboard reads ("?") are refused so a remote host cannot snoop.
      terminal.parser.registerOscHandler(52, (data) => {
        const [targets, payload] = data.split(";", 2);
        if (payload === undefined || payload === "?") return true;
        try {
          const bytes = Uint8Array.from(atob(payload), (character) => character.charCodeAt(0));
          const text = new TextDecoder().decode(bytes);
          const primary = supportsPrimarySelection && !targets.includes("c") && targets.includes("p");
          void writeTerminalClipboard(text, primary).catch(() => {});
        } catch {
          // Malformed base64: ignore, like other terminals.
        }
        return true;
      });
      // Agents (Claude Code, Codex) and Herdr's toasts ask for attention with
      // the bell, OSC 9 (iTerm2; 9;4 is ConEmu progress) or OSC 777 notify.
      terminal.onBell(() => onAttention(paneId));
      terminal.parser.registerOscHandler(9, (data) => {
        if (!data.startsWith("4;")) onAttention(paneId);
        return true;
      });
      terminal.parser.registerOscHandler(777, (data) => {
        if (data.startsWith("notify;")) onAttention(paneId);
        return true;
      });
      searchAddonApi = searchAddon;
      searchAddon.onDidChangeResults((result) => {
        if (disposed) return;
        searchResultIndex = result.resultIndex;
        searchResultCount = result.resultCount;
      });
      terminal.open(container);
      try {
        // GPU rendering keeps full-screen redraws (Herdr, htop) smooth; xterm 6
        // otherwise falls back to the much slower DOM renderer.
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => webgl.dispose());
        terminal.loadAddon(webgl);
      } catch {
        // No WebGL2 in this WebView: the DOM renderer still works.
      }
      const imeTextarea = terminal.textarea;
      if (supportsPrimarySelection && imeTextarea) {
        imeTextarea.style.whiteSpace = "pre";
        handleTerminalCompositionStart = () => {
          resetLinuxTerminalComposition(imeTextarea);
        };
        container.addEventListener(
          "compositionstart",
          handleTerminalCompositionStart,
          true
        );
      }
      container.addEventListener("focusin", activate);
      // Keep the WebView menu from covering menus drawn by terminal applications.
      container.addEventListener("contextmenu", preventContextMenu, true);
      container.addEventListener("wheel", handleZoomWheel, { capture: true, passive: false });
      // Apps that capture the wheel (Claude Code, Codex) otherwise get at most
      // one report per gesture. Send one report per row the finger actually moved.
      handleApplicationWheel = (event: WheelEvent) => {
        if (!mouseTracking.reportsWheel || event.shiftKey || terminalZoomActionFromWheel(event)) return;
        const screen = container.querySelector(".xterm-screen");
        if (!screen || !terminal) return;
        const bounds = screen.getBoundingClientRect();
        const cellWidth = bounds.width / terminal.cols;
        const cellHeight = bounds.height / terminal.rows;
        if (cellWidth <= 0 || cellHeight <= 0) return;
        const lines = mouseTracking.consume(event.deltaY, event.deltaMode, cellHeight, terminal.rows);
        event.preventDefault();
        event.stopImmediatePropagation();
        if (lines === 0) return;
        const position = wheelReportPosition(
          event.clientX,
          event.clientY,
          bounds,
          cellWidth,
          cellHeight
        );
        if (!position) return;
        const report = encodeWheelReports({
          lines,
          position,
          encoding: mouseTracking.encoding,
          cols: terminal.cols,
          rows: terminal.rows,
          ctrl: event.ctrlKey,
          alt: event.altKey,
          shift: false
        });
        if (report) terminal.input(report);
      };
      container.addEventListener("wheel", handleApplicationWheel, { capture: true, passive: false });
      const clearTerminalSelection = () => {
        terminal?.clearSelection();
      };
      const finishTerminalPaste = () => {
        clearTerminalSelection();
      };
      if (supportsPrimarySelection) {
        const suppressMiddleClick = (event: MouseEvent | PointerEvent) => {
          if (event.button !== 1) return;
          event.preventDefault();
          event.stopImmediatePropagation();
        };
        const pastePrimarySelection = async () => {
          if (primarySelectionFrame !== null) {
            window.cancelAnimationFrame(primarySelectionFrame);
            primarySelectionFrame = null;
            const selection = terminal?.getSelection() ?? "";
            if (selection) {
              primarySelectionWrite = primarySelectionWrite
                .then(() => writeTerminalClipboard(selection, true))
                .catch(() => {});
            }
          }
          await primarySelectionWrite;
          const text = await readTerminalClipboard(true);
          if (!disposed && terminal && text) {
            terminal.paste(text);
            finishTerminalPaste();
          }
        };
        handleTerminalMiddlePointerDown = (event: PointerEvent) => {
          if (event.button !== 1) return;
          suppressMiddleClick(event);
          void pastePrimarySelection();
        };
        handleTerminalMiddleMouseEvent = (event: MouseEvent) => {
          suppressMiddleClick(event);
        };
        handleTerminalMiddlePointerUp = (event: PointerEvent) => {
          if (event.button !== 1) return;
          suppressMiddleClick(event);
        };
        handleTerminalAuxClick = (event: MouseEvent) => {
          if (event.button !== 1) return;
          suppressMiddleClick(event);
        };
        container.addEventListener(
          "pointerdown",
          handleTerminalMiddlePointerDown,
          true
        );
        container.addEventListener(
          "mousedown",
          handleTerminalMiddleMouseEvent,
          true
        );
        container.addEventListener(
          "pointerup",
          handleTerminalMiddlePointerUp,
          true
        );
        container.addEventListener("mouseup", handleTerminalMiddleMouseEvent, true);
        container.addEventListener("auxclick", handleTerminalAuxClick, true);
      }
      if (container.clientWidth && container.clientHeight) fitAddon.fit();
      if (snapshot && resumeSessionId) {
        terminal.resize(snapshot.cols, snapshot.rows);
        await new Promise<void>((resolve) => terminal!.write(snapshot.data, resolve));
        shell.atPrompt = snapshot.shellAtPrompt ?? false;
        insideMultiplexer = snapshot.multiplexer ?? false;
      }
      const flushAcknowledgements = () => {
        if (ackTimer !== null) window.clearTimeout(ackTimer);
        ackTimer = null;
        if (!streamId || !ackToken || !acknowledgedBytes) return;
        const bytes = acknowledgedBytes;
        acknowledgedBytes = 0;
        void acknowledgeTerminal(streamId, ackToken, bytes).catch((cause) => {
          if (!disposed) error = String(cause);
        });
      };
      const channel = new Channel<TerminalChannelMessage>();
      const handleTerminalOutput = (bytes: Uint8Array) => {
        if (!terminal) return;
        {
          // Every hop logs to its own file, so anything on the PTY is the
          // real shell — a safe fallback if a log line is ever missed.
          if (connectionState === "connecting" && bytes.length > 0) markConnected();
          terminal.write(bytes, () => {
            acknowledgedBytes += bytes.length;
            if (acknowledgedBytes >= 64 * 1024) flushAcknowledgements();
            else if (ackTimer === null) ackTimer = window.setTimeout(flushAcknowledgements, 16);
          });
          if (host && !operatingSystemDetected) {
            const text = outputDecoder.decode(bytes, { stream: true });
            const stripped = text.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "");
            operatingSystemDetectionBuffer = `${operatingSystemDetectionBuffer}${stripped}`.slice(-16_384);
            const detected = detectHostOperatingSystem(operatingSystemDetectionBuffer);
            if (detected) {
              rememberHostOperatingSystem(host.id, detected);
              operatingSystemDetected = true;
            }
          }
        }
      };
      const handleTerminalEvent = (event: TerminalControlEvent) => {
        if (!terminal) return;
        if (event.kind === "stream") {
          streamId = event.id;
          ackToken = event.token;
        }
        if (event.kind === "checkpoint") {
          terminal.write("", () => {
            if (!terminal || disposed) return;
            flushAcknowledgements();
            const modes = [...extraModes].map(([mode, enabled]) => `\x1b[?${mode}${enabled ? "h" : "l"}`).join("");
            checkpoint?.({ data: serializeAddon.serialize() + modes, rows: terminal.rows, cols: terminal.cols,
              shellAtPrompt: shell.atPrompt, multiplexer: insideMultiplexer });
            checkpoint = null;
          });
        }
        if (event.kind === "hops") setHops(event.labels);
        if (event.kind === "log") {
          recordConnectionLog({
            hop: event.hop,
            level: event.level,
            message: event.message,
            stage: event.stage
          });
        }
        if (event.kind === "error") {
          error = event.message;
          if (connectionState === "connecting") {
            connectionState = "failed";
            connectionError = event.message;
            showConnectionLog = true;
          }
        }
        if (event.kind === "exit") {
          if (connectionState === "connecting") {
            connectionState = "failed";
            if (!connectionError) {
              connectionError = "SSH exited before the session was ready.";
            }
            showConnectionLog = true;
          }
          if (sessionId) {
            onSessionClosed(paneId, sessionId);
            sessionId = null;
          }
        }
        if (event.kind === "disconnect") {
          if (sessionId) {
            onSessionClosed(paneId, sessionId);
            sessionId = null;
          }
          onClose();
        }
      };
      channel.onmessage = (message) => {
        const decoded = decodeTerminalMessage(message);
        if (decoded.kind === "output") handleTerminalOutput(decoded.data);
        else handleTerminalEvent(decoded);
      };

      if (resumeSessionId) {
        try {
          await pendingDetaches.get(resumeSessionId);
          await attachTerminal(resumeSessionId, channel);
          if (disposed) {
            void detachTerminal(resumeSessionId);
            return;
          }
          sessionId = resumeSessionId;
        } catch {
          if (disposed) return;
          const openedSessionId = await openTerminal(terminal.rows, terminal.cols, channel, host, title, initialCwd);
          if (disposed) {
            void closeTerminal(openedSessionId);
            return;
          }
          sessionId = openedSessionId;
        }
      } else {
        const openedSessionId = await openTerminal(terminal.rows, terminal.cols, channel, host, title, initialCwd);
        if (disposed) {
          void closeTerminal(openedSessionId);
          return;
        }
        sessionId = openedSessionId;
      }
      // A resumed PTY still has the source window's geometry.
      let lastPtyRows = resumeSessionId ? -1 : terminal.rows;
      let lastPtyCols = resumeSessionId ? -1 : terminal.cols;
      onSessionReady(paneId, sessionId);
      terminalTransfers.set(paneId, {
        prepare: async () => {
          if (!sessionId) throw new Error("Terminal session has ended");
          flush();
          await writeChain;
          const drained = new Promise<TerminalSnapshot>((resolve) => { checkpoint = resolve; });
          await pauseTerminal(sessionId, true);
          let timer: number | undefined;
          try {
            return await Promise.race([drained, new Promise<never>((_, reject) => {
              timer = window.setTimeout(() => reject(new Error("Terminal snapshot timed out")), 5000);
            })]);
          } finally {
            window.clearTimeout(timer);
            checkpoint = null;
          }
        },
        resume: async () => { if (sessionId) await pauseTerminal(sessionId, false); }
      });
      const encoder = new TextEncoder();
      const cursorPosition = () => terminal
        ? {
            row: terminal.buffer.active.baseY + terminal.buffer.active.cursorY,
            column: terminal.buffer.active.cursorX
          }
        : null;
      // Full-screen and mouse-reporting programs such as Herdr own their input.
      const atShellPrompt = () =>
        shell.editable &&
        terminal?.buffer.active.type === "normal" &&
        terminal.modes.mouseTrackingMode === "none";
      // Password prompts don't echo; keep what is typed there out of the panel.
      const atSecretPrompt = () => {
        const buffer = terminal?.buffer.active;
        const line = buffer?.getLine(buffer.baseY + buffer.cursorY);
        return Boolean(line && isSecretPrompt(line.translateToString(true, 0, buffer!.cursorX)));
      };
      const suggestionsAllowed = () =>
        (atShellPrompt() || (insideMultiplexer && terminal?.buffer.active.type === "alternate")) &&
        !atSecretPrompt();
      terminal.buffer.onBufferChange((buffer) => {
        if (buffer.type === "normal") insideMultiplexer = false;
      });
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
        const bytes = drainTerminalInput(queuedInput);
        queuedInput = [];
        const id = sessionId;
        writeChain = writeChain
          .then(() => writeTerminal(id, bytes))
          .catch((cause: unknown) => {
            error = cause instanceof Error ? cause.message : String(cause);
          });
      };
      const queueData = (data: string) => {
        const chunk = encoder.encode(data);
        queuedInput.push(chunk);
        onUserInput(paneId, chunk);
        if (flushTimer === null) flushTimer = window.setTimeout(flush, 4);
      };
      externalCommandHandler = (command: string, run: boolean) => {
        if (!suggestionsAllowed()) {
          error = "Wait for a shell prompt before inserting a command.";
          return;
        }
        commandStartPosition ??= cursorPosition();
        const data = `${command}${run ? "\r" : ""}`;
        queueData(data);
        const update = updateCommandInput(commandInput, data);
        commandInput = update.input;
        if (update.submitted.length) shell.submit(update.submitted.join("\n"));
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
        if (!suggestionsAllowed()) {
          suggestions = [];
          suggestionIndex = -1;
          return;
        }
        suggestions = buildTerminalSuggestions(
          commandInput,
          snippetCatalog,
          $terminalPreferences.historySuggestions ? commandHistory : [],
          8,
          $terminalPreferences.suggestionMinimumCharacters
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
        if (supportsPrimarySelection) data = normalizeLinuxTerminalInput(data);
        if (isMouseReport(data)) {
          queueData(data);
          if (isMousePress(data)) {
            commandInput = "";
            commandStartPosition = null;
            suggestions = [];
            suggestionIndex = -1;
          }
          return;
        }
        if (!suggestionsAllowed()) {
          queueData(data);
          commandInput = "";
          commandStartPosition = null;
          suggestions = [];
          return;
        }
        if (data.startsWith("\x1b[200~")) {
          queueData(data);
          commandInput += data.slice(6, -6).replaceAll("\r", "\n");
          suggestions = [];
          return;
        }
        const submitting = data.includes("\r") || data.includes("\n");
        const atPrompt = atShellPrompt();
        if (!submitting) commandStartPosition ??= cursorPosition();
        // Inside a multiplexer the rows also hold other panes, so trust typing.
        if (submitting && atPrompt) {
          const rendered = renderedCommand();
          commandInput = reconcileRenderedCommandInput(commandInput, rendered);
        }
        if (suggestions.length > 0) {
          // Application cursor mode (zsh on Debian/Ubuntu) sends ESC O A/B.
          if (/^\x1b[[O][AB]$/.test(data)) {
            const direction = data.endsWith("A") ? -1 : 1;
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
        if (submitting && atPrompt) insideMultiplexer = update.submitted.some(startsMultiplexer);
        if (submitting) shell.submit(update.submitted.join("\n"));
        if (update.submitted.length > 0 || data.includes("\x03") || data.includes("\x15")) {
          commandStartPosition = null;
        }
        refreshSuggestions();
      });
      terminal.onBinary((data) => {
        const chunk = Uint8Array.from(data, (character) => character.charCodeAt(0) & 255);
        queuedInput.push(chunk);
        if (flushTimer === null) flushTimer = window.setTimeout(flush, 4);
      });
      terminal.onSelectionChange(() => {
        if (!supportsPrimarySelection || primarySelectionFrame !== null) return;
        primarySelectionFrame = window.requestAnimationFrame(() => {
          primarySelectionFrame = null;
          const selection = terminal?.getSelection() ?? "";
          if (selection) {
            primarySelectionWrite = primarySelectionWrite
              .then(() => writeTerminalClipboard(selection, true))
              .catch(() => {});
          }
        });
      });
      terminal.attachCustomKeyEventHandler((event) => {
        if (event.type === "keydown" && event.key === "F11" && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
          event.preventDefault();
          event.stopPropagation();
          if (!event.repeat) onFocusModeToggle();
          return false;
        }
        if (
          event.type === "keydown" &&
          event.ctrlKey &&
          event.shiftKey &&
          !event.altKey &&
          event.key.toLowerCase() === "c"
        ) {
          event.preventDefault();
          event.stopPropagation();
          const selection = terminal?.getSelection() ?? "";
          if (selection) void writeTerminalClipboard(selection);
          return false;
        }
        if (
          event.type === "keydown" &&
          event.ctrlKey &&
          event.shiftKey &&
          !event.altKey &&
          event.key.toLowerCase() === "v"
        ) {
          event.preventDefault();
          event.stopPropagation();
          void readTerminalClipboard().then((text) => {
            if (!disposed && terminal && text) {
              terminal.paste(text);
              finishTerminalPaste();
            }
          });
          return false;
        }
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
        if (event.type === "keydown") {
          const zoom = terminalZoomActionFromKeyboard(event);
          if (zoom) {
            event.preventDefault();
            event.stopPropagation();
            applyTerminalZoom(zoom);
            return false;
          }
        }
        if (
          event.type === "keydown" &&
          (event.ctrlKey || event.metaKey) &&
          event.shiftKey &&
          event.key.toLowerCase() === "f"
        ) {
          event.preventDefault();
          openTerminalSearch();
          return false;
        }
        // The window handler toggles terminal tools. Swallow the chord here so
        // xterm does not send it to the shell, but let the keydown bubble.
        if (
          event.ctrlKey &&
          event.altKey &&
          !event.shiftKey &&
          !event.metaKey &&
          event.key.toLowerCase() === "j"
        ) {
          if (event.type === "keydown") event.preventDefault();
          return false;
        }
        return true;
      });

      const syncTerminalSize = () => {
        if (resizeFrame !== null) return;
        resizeFrame = window.requestAnimationFrame(() => {
          resizeFrame = null;
          if (!terminal || !fitAddon || !container.clientWidth || !container.clientHeight) return;
          fitAddon.fit();
          // WebKitGTK shows a WebGL canvas that returns from display:none one
          // draw behind, so output that arrived while the tab was hidden stays
          // invisible until the next write. A second draw two frames later
          // brings the screen up to date.
          terminal.refresh(0, terminal.rows - 1);
          window.requestAnimationFrame(() => window.requestAnimationFrame(() => {
            terminal?.refresh(0, terminal.rows - 1);
          }));
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
      syncTerminalSize();
      terminal.focus();
    }

    void start().catch((cause: unknown) => {
      if (!disposed) error = cause instanceof Error ? cause.message : String(cause);
    });

    return () => {
      const preserveSession = shouldPreserveSession();
      disposed = true;
      terminalTransfers.delete(paneId);
      if (ackTimer !== null) window.clearTimeout(ackTimer);
      if (flushTimer !== null) window.clearTimeout(flushTimer);
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      if (primarySelectionFrame !== null) window.cancelAnimationFrame(primarySelectionFrame);
      if (zoomHintTimer !== null) window.clearTimeout(zoomHintTimer);
      resizeObserver?.disconnect();
      // A pane torn down for a move keeps its session; tell the backend to stop
      // streaming so it is not writing into a webview that has gone away.
      if (preserveSession && sessionId) {
        const id = sessionId;
        const detach: Promise<unknown> = detachTerminal(id)
          .catch(() => {})
          .finally(() => {
            if (pendingDetaches.get(id) === detach) pendingDetaches.delete(id);
          });
        pendingDetaches.set(id, detach);
      }
      container?.removeEventListener("focusin", activate);
      container?.removeEventListener("contextmenu", preventContextMenu, true);
      container?.removeEventListener("wheel", handleZoomWheel, true);
      if (handleApplicationWheel) {
        container?.removeEventListener("wheel", handleApplicationWheel, true);
      }
      if (handleTerminalMiddlePointerDown) {
        container?.removeEventListener(
          "pointerdown",
          handleTerminalMiddlePointerDown,
          true
        );
      }
      if (handleTerminalMiddleMouseEvent) {
        container?.removeEventListener(
          "mousedown",
          handleTerminalMiddleMouseEvent,
          true
        );
        container?.removeEventListener(
          "mouseup",
          handleTerminalMiddleMouseEvent,
          true
        );
      }
      if (handleTerminalMiddlePointerUp) {
        container?.removeEventListener(
          "pointerup",
          handleTerminalMiddlePointerUp,
          true
        );
      }
      if (handleTerminalAuxClick) {
        container?.removeEventListener("auxclick", handleTerminalAuxClick, true);
      }
      if (handleTerminalCompositionStart) {
        container?.removeEventListener(
          "compositionstart",
          handleTerminalCompositionStart,
          true
        );
      }
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
      onSessionClosed(paneId, sessionId);
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
      draggable={headerDraggable}
      onpointerdown={handleHeaderPointerDown}
      ondragstart={onHeaderDragStart}
      ondrag={onHeaderDrag}
      ondragend={onHeaderDragEnd}
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
  {#if zoomHint !== null}
    <div class="terminal-zoom-hint" aria-live="polite">{zoomHint} px</div>
  {/if}
  {#if searchOpen}
    <div
      class="terminal-search"
      class:expanded={searchSettingsOpen}
      role="search"
      onpointerdown={(event) => event.stopPropagation()}
    >
      <div class="terminal-search-bar">
        <Icon name="search" size={15} />
        <input
          bind:this={searchInput}
          class:invalid={Boolean(searchError)}
          value={searchQuery}
          aria-label="Find in terminal"
          aria-invalid={Boolean(searchError)}
          title={searchError}
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
        <button
          class="terminal-search-settings-toggle"
          class:active={searchSettingsOpen}
          title="Search settings"
          aria-label="Search settings"
          aria-expanded={searchSettingsOpen}
          onclick={() => searchSettingsOpen = !searchSettingsOpen}
        ><Icon name="appearance" size={15} /></button>
        <button title="Close (Escape)" onclick={closeTerminalSearch}>
          <Icon name="close" size={14} />
        </button>
      </div>
      {#if searchSettingsOpen}
        <div class="terminal-search-options">
          <label>
            <input
              type="checkbox"
              checked={searchCaseSensitive}
              onchange={() => {
                searchCaseSensitive = !searchCaseSensitive;
                refreshTerminalSearch();
              }}
            />
            <span>Match case</span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={searchWholeWord}
              onchange={() => {
                searchWholeWord = !searchWholeWord;
                refreshTerminalSearch();
              }}
            />
            <span>Match entire word only</span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={searchRegex}
              onchange={() => {
                searchRegex = !searchRegex;
                refreshTerminalSearch();
              }}
            />
            <span>Match as regular expression</span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={searchWrapAround}
              onchange={() => searchWrapAround = !searchWrapAround}
            />
            <span>Wrap around</span>
          </label>
          {#if searchError}<small role="alert">{searchError}</small>{/if}
        </div>
      {/if}
    </div>
  {/if}
  <div class="terminal-mount" bind:this={container}></div>
  {#if remoteSession && connectionState !== "ready"}
    <div
      class="terminal-connecting"
      class:failed={connectionState === "failed"}
      role="status"
      aria-live="polite"
    >
      <div class="terminal-connecting-card">
        <header>
          <span class="host-badge terminal-connecting-badge {host?.color ?? 'slate'}">
            <HostIcon hostId={host?.id} size={26} />
          </span>
          <span class="terminal-connecting-title">
            <strong>{host?.label ?? title}</strong>
            <small>SSH {host?.username}@{host?.address}:{host?.port}</small>
          </span>
          <button
            class="terminal-connecting-logs"
            aria-expanded={showConnectionLog}
            onclick={() => (showConnectionLog = !showConnectionLog)}
          >{showConnectionLog ? "Hide logs" : "Show logs"}</button>
        </header>

        <div class="terminal-connecting-track" aria-label="Connection progress">
          <span class="terminal-connecting-node origin">
            <Icon name="connect" size={16} />
          </span>
          {#each hops as hop, index (index)}
            <span
              class="terminal-connecting-rail"
              class:failed={hopFailed(index)}
              style={`--connection-progress:${Math.round(hopProgress(index) * 100)}%`}
            ><i></i></span>
            <span
              class="terminal-connecting-node"
              class:reached={hopReached(index)}
              class:failed={hopFailed(index)}
              class:pending={!hopReached(index) && index === activeHop}
              title={hop}
            >
              <Icon name={index === hops.length - 1 ? "terminal" : "server"} size={16} />
              {#if hops.length > 1}<small>{hop}</small>{/if}
            </span>
          {/each}
        </div>

        <p class="terminal-connecting-status">
          {connectionHeadline}{#if connectionSubhead}<span> {connectionSubhead}</span>{/if}
        </p>
        {#if connectionError}
          <p class="terminal-connecting-error">{connectionError}</p>
        {/if}

        {#if showConnectionLog}
          <div class="terminal-connecting-log" bind:this={logScroller} tabindex="-1">
            {#if connectionLog.length === 0}
              <p class="terminal-connecting-log-empty">Waiting for OpenSSH…</p>
            {:else}
              {#each connectionLog as entry, index (index)}
                <p class="log-{entry.level}">
                  <Icon name={connectionLogIcons[entry.level]} size={14} />
                  <span>
                    {#if hops.length > 1}<em>{hops[entry.hop] ?? `hop ${entry.hop + 1}`}</em>{/if}
                    {entry.message}
                  </span>
                </p>
              {/each}
            {/if}
          </div>
        {/if}

        <footer>
          {#if connectionLog.length > 0}
            <button
              class="terminal-connecting-copy"
              onclick={() => void writeTerminalClipboard(connectionLogText())}
            >Copy log</button>
          {/if}
          <button class="terminal-connecting-close" onclick={onClose}>Close</button>
          <button class="terminal-connecting-retry" onclick={onRetry}>Start over</button>
        </footer>
      </div>
    </div>
  {/if}
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
