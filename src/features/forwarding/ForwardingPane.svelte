<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import CustomSelect from "../../components/CustomSelect.svelte";
  import Icon from "../../components/Icon.svelte";
  import { alertDialog, confirmDialog } from "../../lib/dialog";
  import { beginMarqueeSelection } from "../../lib/marqueeSelection";
  import {
    deletePortForward,
    listHosts,
    listPortForwards,
    platformCapabilities,
    savePortForward,
    startTunnel,
    stopTunnelPortProcesses,
    stopTunnel,
    tunnelPortProcesses,
    tunnelStates
  } from "../../lib/ipc";
  import type { ForwardKind, Host, PlatformCapabilities, PortForward } from "../../lib/types";
  import {
    confirmDiscardChanges,
    draftChanged,
    draftSnapshot
  } from "../../lib/unsavedChanges";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  let { ondirtychange = (_dirty: boolean) => {} } = $props<{
    ondirtychange?: (dirty: boolean) => void;
  }>();

  let hosts = $state<Host[]>([]);
  let rules = $state<PortForward[]>([]);
  let selected = $state<PortForward | null>(null);
  let focusedRuleId = $state<string | null>(null);
  let ruleContextMenu = $state<{ rule: PortForward; x: number; y: number } | null>(null);
  let bulkRuleContextMenu = $state<{ x: number; y: number } | null>(null);
  let selectedRuleIds = $state<string[]>([]);
  let runningIds = $state<Set<string>>(new Set());
  let loading = $state(true);
  let busy = $state(false);
  let message = $state("");
  let isError = $state(false);
  let inspectorMenuOpen = $state(false);
  let collectionView = $state<CollectionView>("grid");
  let collectionSort = $state<CollectionSort>("az");
  let collectionQuery = $state("");
  let editorBaseline = $state<string | null>(null);
  let capabilities = $state<PlatformCapabilities>({
    localPermissionEditing: false,
    customApplicationOpen: false,
    foreignProcessTermination: false,
    sshAvailable: false,
    sshKeygenAvailable: false,
    localShell: null
  });
  const activePortConflictDialogs = new Set<string>();
  /** Rules with a start/stop in flight, from either the user or the poll. */
  const rulesInFlight = new Set<string>();
  let statesRefreshPromise: Promise<void> | null = null;
  const editorDirty = $derived(Boolean(selected) && draftChanged(editorBaseline, selected));
  const visibleRules = $derived.by(() =>
    rules
      .filter((rule) => {
        const host = hosts.find((candidate) => candidate.id === rule.host_id);
        return `${rule.name} ${rule.kind} ${rule.bind_host} ${rule.bind_port} ${rule.destination_host ?? ""} ${rule.destination_port ?? ""} ${host?.label ?? ""}`
          .toLowerCase()
          .includes(collectionQuery.trim().toLowerCase());
      })
      .sort((left, right) =>
        compareCollectionItems(
          left.name,
          right.name,
          left.created_at,
          right.created_at,
          collectionSort
        )
      )
  );

  onMount(() => {
    void refresh();
    const timer = window.setInterval(refreshStates, 2000);
    return () => {
      window.clearInterval(timer);
      ondirtychange(false);
    };
  });

  $effect(() => ondirtychange(editorDirty));

  async function refresh(preferredId?: string) {
    loading = true;
    try {
      [hosts, rules, capabilities] = await Promise.all([
        listHosts(),
        listPortForwards(),
        platformCapabilities()
      ]);
      const next = preferredId
        ? rules.find((rule) => rule.id === preferredId) ?? null
        : selected
          ? rules.find((rule) => rule.id === selected?.id) ?? null
          : null;
      selected = next ? { ...next } : null;
      if (preferredId) focusedRuleId = preferredId;
      else if (focusedRuleId && !rules.some((rule) => rule.id === focusedRuleId)) focusedRuleId = null;
      editorBaseline = selected ? draftSnapshot(selected) : null;
      await refreshStates();
    } catch (cause) {
      showError(cause);
    } finally {
      loading = false;
    }
  }

  /**
   * Reads tunnel state, coalescing concurrent polls.
   *
   * `fresh` callers need state observed *after* their own start/stop landed, so
   * they wait for any in-flight read to finish and then issue a new one; being
   * handed a poll that began earlier made a tunnel that had just started look
   * as though SSH had closed it.
   */
  function refreshStates(fresh = false): Promise<void> {
    if (statesRefreshPromise) {
      if (!fresh) return statesRefreshPromise;
      return statesRefreshPromise.then(() => refreshStates(true));
    }
    const refreshPromise = (async () => {
      try {
      const states = await tunnelStates();
      const previouslyRunning = runningIds;
      const nextRunningIds = new Set(states.filter((state) => state.running).map((state) => state.id));
      if (
        nextRunningIds.size !== runningIds.size ||
        [...nextRunningIds].some((id) => !runningIds.has(id))
      ) {
        runningIds = nextRunningIds;
      }
      // The backend reports an exited tunnel exactly once and then forgets it,
      // so anything not surfaced here is lost for good.
      const failures = states.filter((state) => !state.running && state.exitCode !== 0);
      const failed = failures[0];
      if (failures.length > 1) {
        console.warn("Multiple tunnels stopped", failures);
      }
      void previouslyRunning;
      if (failed) {
        const failureMessage =
          failed.message ||
          `SSH forwarding stopped${failed.exitCode === null ? "" : ` with exit code ${failed.exitCode}`}.`;
        const rule = rules.find((candidate) => candidate.id === failed.id);
        if (
          rule &&
          rule.kind !== "remote" &&
          failureMessage.toLowerCase().includes("address already in use")
        ) {
          message = "";
          isError = false;
          void resolveLocalPortConflict(rule);
        } else {
          message = failureMessage;
          isError = true;
        }
      }
      } catch {
        // The next explicit action will surface a useful error.
      }
    })();
    statesRefreshPromise = refreshPromise;
    return refreshPromise.finally(() => {
      if (statesRefreshPromise === refreshPromise) statesRefreshPromise = null;
    });
  }

  async function resolveLocalPortConflict(rule: PortForward) {
    if (activePortConflictDialogs.has(rule.id)) return;
    activePortConflictDialogs.add(rule.id);
    try {
      const processes = await tunnelPortProcesses(rule.bind_port);
      const processDetails = processes.length > 0
        ? processes
            .map((process) =>
              `${process.name} (PID ${process.pid})${process.requiresElevation ? " — administrator permission required" : ""}`
            )
            .join("\n")
        : "The owning process is protected, so administrator permission is required to identify and stop it.";
      if (!capabilities.foreignProcessTermination) {
        await alertDialog({
          title: `Port ${rule.bind_port} is already in use`,
          message:
            `${processDetails}\n\nClose the process in Task Manager, then start “${rule.name}” again.`,
          confirmLabel: "Close"
        });
        return;
      }
      const accepted = await confirmDialog({
        title: `Port ${rule.bind_port} is already in use`,
        message:
          `${processDetails}\n\nStop the process using ${rule.bind_host}:${rule.bind_port} and retry “${rule.name}”? ` +
          "The process may have unsaved work. Heminus never stores administrator passwords.",
        confirmLabel: "Stop process & retry",
        danger: true
      });
      if (!accepted) return;

      busy = true;
      rulesInFlight.add(rule.id);
      await stopTunnelPortProcesses(rule.bind_port, processes.map((process) => process.pid));
      const host = hosts.find((candidate) => candidate.id === rule.host_id);
      if (!host) throw new Error("The SSH host for this tunnel no longer exists.");
      await startTunnel(rule, host);
      await new Promise((resolve) => window.setTimeout(resolve, 180));
      await refreshStates(true);
      if (runningIds.has(rule.id)) {
        message = `Stopped the process using port ${rule.bind_port} and started the tunnel.`;
        isError = false;
      } else if (!isError) {
        throw new Error("SSH stopped again after the port was released.");
      }
    } catch (cause) {
      message = "";
      isError = false;
      await alertDialog({
        title: "Could not start the tunnel",
        message: cause instanceof Error ? cause.message : String(cause),
        confirmLabel: "Close"
      });
    } finally {
      rulesInFlight.delete(rule.id);
      busy = false;
      activePortConflictDialogs.delete(rule.id);
    }
  }

  async function createRule(kind: ForwardKind = "local") {
    if (!(await confirmDiscardChanges(editorDirty))) return;
    if (hosts.length === 0) {
      message = "Create a host before adding a forwarding rule.";
      isError = true;
      return;
    }
    selected = {
      id: crypto.randomUUID(),
      name: "",
      kind,
      bind_host: "127.0.0.1",
      bind_port: kind === "dynamic" ? 1080 : 8080,
      destination_host: kind === "dynamic" ? null : "127.0.0.1",
      destination_port: kind === "dynamic" ? null : 80,
      host_id: hosts[0].id,
      enabled: false,
      created_at: new Date().toISOString()
    };
    focusedRuleId = selected.id;
    inspectorMenuOpen = false;
    message = "Forwarding uses the host and identity stored in the Heminus vault.";
    isError = false;
    editorBaseline = draftSnapshot(selected);
  }

  async function selectRule(rule: PortForward) {
    if (selected?.id === rule.id) {
      focusedRuleId = rule.id;
      return;
    }
    if (selected && !(await confirmDiscardChanges(editorDirty))) return;
    selected = null;
    editorBaseline = null;
    inspectorMenuOpen = false;
    focusedRuleId = rule.id;
    message = "";
  }

  async function handleRuleClick(event: MouseEvent, rule: PortForward) {
    if (event.ctrlKey || event.metaKey) {
      selectedRuleIds = selectedRuleIds.includes(rule.id)
        ? selectedRuleIds.filter((id) => id !== rule.id)
        : [...selectedRuleIds, rule.id];
      focusedRuleId = selectedRuleIds.at(-1) ?? null;
      return;
    }
    selectedRuleIds = [rule.id];
    await selectRule(rule);
  }

  function startRuleMarquee(event: PointerEvent) {
    beginMarqueeSelection(
      event,
      event.currentTarget as HTMLElement,
      selectedRuleIds,
      (ids) => {
        selectedRuleIds = ids;
        focusedRuleId = ids.at(-1) ?? null;
      }
    );
  }

  async function editRule(rule: PortForward) {
    if (selected?.id !== rule.id && !(await confirmDiscardChanges(editorDirty))) return;
    selected = { ...rule };
    focusedRuleId = rule.id;
    inspectorMenuOpen = false;
    message = "";
    editorBaseline = draftSnapshot(selected);
  }

  async function closeInspector() {
    if (!(await confirmDiscardChanges(editorDirty))) return;
    selected = null;
    editorBaseline = null;
    inspectorMenuOpen = false;
    message = "";
  }

  function openRuleContextMenu(event: MouseEvent, rule: PortForward) {
    event.preventDefault();
    event.stopPropagation();
    if (selectedRuleIds.length > 1 && selectedRuleIds.includes(rule.id)) {
      ruleContextMenu = null;
      bulkRuleContextMenu = {
        x: Math.max(10, Math.min(event.clientX, window.innerWidth - 220)),
        y: Math.max(10, Math.min(event.clientY, window.innerHeight - 220))
      };
      return;
    }
    selectedRuleIds = [rule.id];
    bulkRuleContextMenu = null;
    focusedRuleId = rule.id;
    ruleContextMenu = {
      rule: { ...rule },
      x: Math.max(10, Math.min(event.clientX, window.innerWidth - 238)),
      y: Math.max(10, Math.min(event.clientY, window.innerHeight - 194))
    };
  }

  function selectedRules(): PortForward[] {
    const ids = new Set(selectedRuleIds);
    return rules.filter((rule) => ids.has(rule.id));
  }

  function takeContextRule(): PortForward | null {
    const rule = ruleContextMenu?.rule ?? null;
    ruleContextMenu = null;
    return rule;
  }

  function changeKind(kind: ForwardKind) {
    if (!selected) return;
    selected.kind = kind;
    if (kind === "dynamic") {
      selected.destination_host = null;
      selected.destination_port = null;
    } else {
      selected.destination_host ??= "127.0.0.1";
      selected.destination_port ??= 80;
    }
  }

  function forwardKindLetter(kind: ForwardKind): string {
    return kind === "local" ? "L" : kind === "remote" ? "R" : "D";
  }

  function forwardingRoute(rule: PortForward): string {
    const host = hosts.find((candidate) => candidate.id === rule.host_id);
    const hostLabel = host?.label ?? "SSH host";
    const bindAddress = ["127.0.0.1", "localhost", "::1"].includes(rule.bind_host)
      ? `${rule.kind === "dynamic" ? "SOCKS" : "Local"}:${rule.bind_port}`
      : `${rule.bind_host}:${rule.bind_port}`;
    if (rule.kind === "dynamic") return `${bindAddress} → ${hostLabel} → Internet`;
    const destination = `${rule.destination_host ?? "Destination"}:${rule.destination_port ?? "—"}`;
    if (rule.kind === "remote") {
      return `Port ${rule.bind_port} on ${hostLabel} → This device → ${destination}`;
    }
    return `${bindAddress} → ${hostLabel} → ${destination}`;
  }

  async function persist(): Promise<PortForward | null> {
    if (!selected || busy) return null;
    busy = true;
    try {
      const saved = await savePortForward(selected);
      await refresh(saved.id);
      message = "Forwarding rule saved";
      isError = false;
      return saved;
    } catch (cause) {
      showError(cause);
      return null;
    } finally {
      busy = false;
    }
  }

  async function toggleRule(rule: PortForward) {
    // `busy` alone was not enough: resolveLocalPortConflict runs unawaited from
    // the poll with busy still false, so the same tunnel could be started twice.
    if (busy || rulesInFlight.has(rule.id)) return;
    busy = true;
    rulesInFlight.add(rule.id);
    try {
      const draft = selected?.id === rule.id ? selected : rule;
      const saved = await savePortForward(draft);
      rules = rules.map((item) => item.id === saved.id ? { ...saved } : item);
      if (selected?.id === saved.id) {
        selected = { ...saved };
        editorBaseline = draftSnapshot(selected);
      }
      focusedRuleId = saved.id;
      if (runningIds.has(saved.id)) {
        await stopTunnel(saved.id);
        message = "Tunnel stopped";
      } else {
        const host = hosts.find((candidate) => candidate.id === saved.host_id);
        if (!host) throw new Error("Select a valid SSH host");
        await startTunnel(saved, host);
        message = "Tunnel started";
      }
      isError = false;
      await new Promise((resolve) => window.setTimeout(resolve, 180));
      await refreshStates(true);
      if (!runningIds.has(saved.id) && message === "Tunnel started" && !isError) {
        throw new Error("SSH closed the tunnel. Connect to the host once and verify your key or agent.");
      }
    } catch (cause) {
      showError(cause);
      await refreshStates();
    } finally {
      busy = false;
    }
  }

  async function duplicateRule(rule: PortForward) {
    if (busy) return;
    busy = true;
    try {
      const duplicate = await savePortForward({
        ...rule,
        id: crypto.randomUUID(),
        name: `${rule.name} copy`,
        enabled: false,
        created_at: new Date().toISOString()
      });
      await refresh(duplicate.id);
      message = "Forwarding rule duplicated";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      busy = false;
    }
  }

  async function removeRule(rule: PortForward) {
    if (!rules.some((item) => item.id === rule.id)) return;
    if (
      !(await confirmDialog({
        title: "Remove forwarding rule?",
        message: `Remove “${rule.name}”?`,
        confirmLabel: "Remove",
        danger: true
      }))
    ) return;
    try {
      await stopTunnel(rule.id);
      await deletePortForward(rule.id);
      if (selected?.id === rule.id) selected = null;
      if (focusedRuleId === rule.id) focusedRuleId = null;
      await refresh();
    } catch (cause) {
      showError(cause);
    }
  }

  async function toggleSelectedRules(start: boolean) {
    bulkRuleContextMenu = null;
    for (const rule of selectedRules()) {
      if (runningIds.has(rule.id) !== start) await toggleRule(rule);
    }
  }

  async function duplicateSelectedRules() {
    const targets = selectedRules();
    bulkRuleContextMenu = null;
    if (!targets.length || busy) return;
    busy = true;
    try {
      const duplicates = await Promise.all(targets.map((rule) => savePortForward({
        ...rule,
        id: crypto.randomUUID(),
        name: `${rule.name} copy`,
        enabled: false,
        created_at: new Date().toISOString()
      })));
      await refresh();
      selectedRuleIds = duplicates.map((rule) => rule.id);
      message = `${duplicates.length} forwarding rules duplicated`;
    } catch (cause) {
      showError(cause);
    } finally {
      busy = false;
    }
  }

  async function removeSelectedRules() {
    const targets = selectedRules();
    bulkRuleContextMenu = null;
    if (!targets.length || !(await confirmDialog({
      title: `Remove ${targets.length} forwarding rules?`,
      message: "Every selected tunnel will be stopped and removed.",
      confirmLabel: "Remove all",
      danger: true
    }))) return;
    try {
      await Promise.all(targets.map(async (rule) => {
        await stopTunnel(rule.id);
        await deletePortForward(rule.id);
      }));
      if (selected && selectedRuleIds.includes(selected.id)) selected = null;
      selectedRuleIds = [];
      await refresh();
    } catch (cause) {
      showError(cause);
    }
  }

  function showError(cause: unknown) {
    message = cause instanceof Error ? cause.message : String(cause);
    isError = true;
  }
</script>

<svelte:window
  onclick={() => {
    ruleContextMenu = null;
    bulkRuleContextMenu = null;
  }}
  onkeydown={(event) => {
    if (event.key === "Escape") {
      ruleContextMenu = null;
      bulkRuleContextMenu = null;
    }
  }}
/>

<section class="manager-page" class:with-inspector={Boolean(selected)}>
  <div class="manager-main">
    <header class="content-toolbar">
      <button class="secondary-action" onclick={() => createRule()}><Icon name="plus" /> New forwarding</button>
      <div class="toolbar-spacer"></div>
      <CollectionControls
        view={collectionView}
        sort={collectionSort}
        onviewchange={(view) => (collectionView = view)}
        onsortchange={(sort) => (collectionSort = sort)}
        searchValue={collectionQuery}
        searchPlaceholder="Search forwarding"
        onsearchchange={(value) => (collectionQuery = value)}
      />
      <span class="manager-count">{runningIds.size} active</span>
    </header>
    <div
      class="manager-scroll"
      role="region"
      aria-label="Port forwarding selection area"
      onpointerdown={startRuleMarquee}
    >
      <div class="page-heading">
        <div><h1>Port Forwarding</h1><p>Reach private services through trusted SSH hosts.</p></div>
      </div>
      {#if message && isError}
        <div class="error-row forwarding-error-row">{message}</div>
      {/if}
      {#if loading}
        <div class="loading-row">Loading forwarding rules…</div>
      {:else if rules.length === 0}
        <div class="empty-page manager-empty">
          <div class="empty-glyph"><Icon name="forward" size={30} /></div>
          <h2>Create a secure tunnel</h2>
          <p>Local, remote and dynamic SOCKS forwarding are supported.</p>
          <button class="secondary-action" onclick={() => createRule()}>New forwarding</button>
        </div>
      {:else if visibleRules.length === 0}
        <div class="empty-page manager-empty">
          <div class="empty-glyph"><Icon name="search" size={28} /></div>
          <h2>No matching forwarding rules</h2>
          <p>Try another name, endpoint, or host.</p>
        </div>
      {:else}
        <div
          class="manager-grid forwarding-grid selectable-grid"
          role="list"
          class:list-view={collectionView === "list"}
        >
          {#each visibleRules as rule (rule.id)}
            <div
              class="manager-card-shell"
              role="group"
              data-select-id={rule.id}
              oncontextmenu={(event) => openRuleContextMenu(event, rule)}
            >
              <button
                class="manager-card forwarding-card"
                class:selected={selectedRuleIds.includes(rule.id)}
                class:running-card={runningIds.has(rule.id)}
                onclick={(event) => void handleRuleClick(event, rule)}
                ondblclick={() => void toggleRule(rule)}
              >
                <span class="manager-icon forwarding-kind-icon">{forwardKindLetter(rule.kind)}</span>
                <span>
                  <strong>{rule.name}</strong>
                  <small>{forwardingRoute(rule)}</small>
                </span>
              </button>
              <button
                class="manager-card-edit"
                title={`Edit ${rule.name}`}
                onclick={(event) => {
                  event.stopPropagation();
                  void editRule(rule);
                }}
              ><Icon name="edit" size={17} /></button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  {#if selected}
  <aside class="manager-inspector">
    <header>
      <div class="manager-inspector-heading">
        <strong>{selected?.name || "New Forwarding"}</strong><span>Personal vault</span>
      </div>
      <div class="manager-inspector-header-actions">
        {#if selected && rules.some((rule) => rule.id === selected?.id)}
          <div class="host-editor-menu-anchor">
            <button
              class="icon-button"
              class:active={inspectorMenuOpen}
              title="Forwarding actions"
              onclick={() => (inspectorMenuOpen = !inspectorMenuOpen)}
            ><Icon name="more" /></button>
            {#if inspectorMenuOpen}
              <div class="host-editor-menu">
                <button onclick={() => selected && void duplicateRule(selected)}>
                  <Icon name="copy" /> Duplicate
                </button>
                <button class="danger-copy" onclick={() => selected && void removeRule(selected)}>
                  <Icon name="trash" /> Remove
                </button>
              </div>
            {/if}
          </div>
        {/if}
        {#if selected}
          <button class="icon-button" title="Close editor" onclick={closeInspector}>
            <Icon name="close" />
          </button>
        {/if}
      </div>
    </header>
    {#if selected}
      <div class="inspector-scroll">
        <section class="form-card">
          <h2>SSH tunnel</h2>
          <div class="segmented">
            {#each ["local", "remote", "dynamic"] as kind}
              <button
                class:active={selected.kind === kind}
                onclick={() => changeKind(kind as ForwardKind)}
              >{kind[0].toUpperCase() + kind.slice(1)}</button>
            {/each}
          </div>
          <p class="forward-kind-help">
            {selected.kind === "local"
              ? "Expose a remote service on a port of this computer."
              : selected.kind === "remote"
                ? "Expose a local or private service on the remote SSH host."
                : "Create a local SOCKS proxy through the intermediate SSH host."}
          </p>
          <label for="forward-name">Name</label>
          <input id="forward-name" bind:value={selected.name} placeholder="Development database" />

          {#if selected.kind === "remote"}
            <span class="field-label">Remote host</span>
            <CustomSelect
              value={selected.host_id}
              options={hosts.map((host) => ({ value: host.id, label: host.label }))}
              ariaLabel="Remote host"
              onchange={(value) => selected && (selected.host_id = value)}
            />
          {/if}

          <div class="forward-field-grid port-first">
            <div>
              <label for="forward-bind-port">
                {selected.kind === "remote" ? "Remote port number" : "Local port number"}
              </label>
              <input
                id="forward-bind-port"
                type="number"
                min="1"
                max="65535"
                bind:value={selected.bind_port}
              />
            </div>
            <div>
              <label for="forward-bind-address">Bind address</label>
              <input
                id="forward-bind-address"
                bind:value={selected.bind_host}
                placeholder="127.0.0.1"
              />
            </div>
          </div>

          {#if selected.kind !== "remote"}
            <span class="field-label">Intermediate host</span>
            <CustomSelect
              value={selected.host_id}
              options={hosts.map((host) => ({ value: host.id, label: host.label }))}
              ariaLabel="Intermediate host"
              onchange={(value) => selected && (selected.host_id = value)}
            />
          {/if}

          {#if selected.kind !== "dynamic"}
            <div class="forward-field-grid address-first">
              <div>
                <label for="forward-destination-address">Destination address</label>
                <input
                  id="forward-destination-address"
                  value={selected.destination_host ?? ""}
                  placeholder="127.0.0.1"
                  oninput={(event) =>
                    selected && (selected.destination_host = event.currentTarget.value || null)}
                />
              </div>
              <div>
                <label for="forward-destination-port">Destination port number</label>
                <input
                  id="forward-destination-port"
                  type="number"
                  min="1"
                  max="65535"
                  value={selected.destination_port ?? ""}
                  oninput={(event) =>
                    selected &&
                    (selected.destination_port = event.currentTarget.valueAsNumber || null)}
                />
              </div>
            </div>
          {/if}
        </section>
        {#if message}<p class:error-message={isError} class="form-message">{message}</p>{/if}
      </div>
      <footer class="manager-inspector-footer">
        <button class="manager-primary-action" disabled={busy} onclick={persist}>
          <Icon name="save" /> {busy ? "Saving…" : "Save"}
        </button>
      </footer>
    {:else}
      <div class="empty-inspector">
        <Icon name="forward" size={32} />
        <p>Select a rule or create a new one.</p>
      </div>
    {/if}
  </aside>
  {/if}

  {#if ruleContextMenu}
    <div
      class="host-context-menu manager-context-menu"
      style:left={`${ruleContextMenu.x}px`}
      style:top={`${ruleContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <button
        role="menuitem"
        onclick={() => {
          const rule = takeContextRule();
          if (rule) void toggleRule(rule);
        }}
      ><Icon name="connect" size={17} /><span>{runningIds.has(ruleContextMenu.rule.id) ? "Stop Tunnel" : "Start Tunnel"}</span></button>
      <button
        role="menuitem"
        onclick={() => {
          const rule = takeContextRule();
          if (rule) void editRule(rule);
        }}
      ><Icon name="edit" size={17} /><span>Edit</span></button>
      <button
        role="menuitem"
        onclick={() => {
          const rule = takeContextRule();
          if (rule) void duplicateRule(rule);
        }}
      ><Icon name="copy" size={17} /><span>Duplicate</span></button>
      <div class="context-separator"></div>
      <button
        class="danger"
        role="menuitem"
        onclick={() => {
          const rule = takeContextRule();
          if (rule) void removeRule(rule);
        }}
      ><Icon name="trash" size={17} /><span>Remove</span></button>
    </div>
  {/if}

  {#if bulkRuleContextMenu && selectedRuleIds.length > 1}
    <div
      class="host-context-menu manager-context-menu"
      style:left={`${bulkRuleContextMenu.x}px`}
      style:top={`${bulkRuleContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <div class="bulk-menu-heading">{selectedRuleIds.length} tunnels selected</div>
      <button role="menuitem" onclick={() => void toggleSelectedRules(true)}>
        <Icon name="connect" size={17} /><span>Start all</span>
      </button>
      <button role="menuitem" onclick={() => void toggleSelectedRules(false)}>
        <Icon name="close" size={17} /><span>Stop all</span>
      </button>
      <button role="menuitem" onclick={() => void duplicateSelectedRules()}>
        <Icon name="copy" size={17} /><span>Duplicate all</span>
      </button>
      <div class="context-separator"></div>
      <button class="danger" role="menuitem" onclick={() => void removeSelectedRules()}>
        <Icon name="trash" size={17} /><span>Remove all</span>
      </button>
    </div>
  {/if}
</section>
