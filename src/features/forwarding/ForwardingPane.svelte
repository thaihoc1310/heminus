<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import CustomSelect from "../../components/CustomSelect.svelte";
  import Icon from "../../components/Icon.svelte";
  import { confirmDialog } from "../../lib/dialog";
  import {
    deletePortForward,
    listHosts,
    listPortForwards,
    savePortForward,
    startTunnel,
    stopTunnel,
    tunnelStates
  } from "../../lib/ipc";
  import type { ForwardKind, Host, PortForward } from "../../lib/types";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  let hosts = $state<Host[]>([]);
  let rules = $state<PortForward[]>([]);
  let selected = $state<PortForward | null>(null);
  let runningIds = $state<Set<string>>(new Set());
  let loading = $state(true);
  let busy = $state(false);
  let message = $state("");
  let isError = $state(false);
  let inspectorMenuOpen = $state(false);
  let collectionView = $state<CollectionView>("grid");
  let collectionSort = $state<CollectionSort>("az");
  let collectionQuery = $state("");
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
    return () => window.clearInterval(timer);
  });

  async function refresh(preferredId?: string) {
    loading = true;
    try {
      [hosts, rules] = await Promise.all([listHosts(), listPortForwards()]);
      const next = preferredId
        ? rules.find((rule) => rule.id === preferredId) ?? null
        : selected
          ? rules.find((rule) => rule.id === selected?.id) ?? null
          : null;
      selected = next ? { ...next } : null;
      await refreshStates();
    } catch (cause) {
      showError(cause);
    } finally {
      loading = false;
    }
  }

  async function refreshStates() {
    try {
      const states = await tunnelStates();
      runningIds = new Set(states.filter((state) => state.running).map((state) => state.id));
      const failed = states.find(
        (state) => !state.running && state.id === selected?.id && state.exitCode !== 0
      );
      if (failed) {
        message =
          failed.message ||
          `SSH forwarding stopped${failed.exitCode === null ? "" : ` with exit code ${failed.exitCode}`}.`;
        isError = true;
      }
    } catch {
      // The next explicit action will surface a useful error.
    }
  }

  function createRule(kind: ForwardKind = "local") {
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
    inspectorMenuOpen = false;
    message = "Forwarding uses the host and identity stored in the Heminus vault.";
    isError = false;
  }

  function chooseRule(rule: PortForward) {
    selected = { ...rule };
    inspectorMenuOpen = false;
    message = "";
  }

  function closeInspector() {
    selected = null;
    inspectorMenuOpen = false;
    message = "";
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

  async function persist() {
    if (!selected || busy) return;
    busy = true;
    try {
      const saved = await savePortForward(selected);
      await refresh(saved.id);
      message = "Forwarding rule saved";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      busy = false;
    }
  }

  async function toggle() {
    if (!selected || busy) return;
    busy = true;
    try {
      if (runningIds.has(selected.id)) {
        await stopTunnel(selected.id);
        message = "Tunnel stopped";
      } else {
        const host = hosts.find((candidate) => candidate.id === selected?.host_id);
        if (!host) throw new Error("Select a valid SSH host");
        const saved = await savePortForward(selected);
        selected = { ...saved };
        await startTunnel(saved, host);
        message = "Tunnel started";
      }
      isError = false;
      await new Promise((resolve) => window.setTimeout(resolve, 180));
      await refreshStates();
      if (!runningIds.has(selected.id) && message === "Tunnel started" && !isError) {
        throw new Error("SSH closed the tunnel. Connect to the host once and verify your key or agent.");
      }
    } catch (cause) {
      showError(cause);
      await refreshStates();
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!selected || !rules.some((rule) => rule.id === selected?.id)) return;
    if (
      !(await confirmDialog({
        title: "Remove forwarding rule?",
        message: `Remove “${selected.name}”?`,
        confirmLabel: "Remove",
        danger: true
      }))
    ) return;
    try {
      await stopTunnel(selected.id);
      await deletePortForward(selected.id);
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
    <div class="manager-scroll">
      <div class="page-heading">
        <div><h1>Port Forwarding</h1><p>Reach private services through trusted SSH hosts.</p></div>
      </div>
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
        <div class="manager-grid" class:list-view={collectionView === "list"}>
          {#each visibleRules as rule (rule.id)}
            <button
              class="manager-card"
              class:selected={selected?.id === rule.id}
              onclick={() => chooseRule(rule)}
            >
              <span class="manager-icon"><Icon name="forward" /></span>
              <span>
                <strong>{rule.name}</strong>
                <small>{rule.kind} · {rule.bind_host}:{rule.bind_port}</small>
              </span>
              <span class:running={runningIds.has(rule.id)} class="runtime-dot"></span>
            </button>
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
                <button class="danger-copy" onclick={() => void remove()}>
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
      <footer class="manager-inspector-footer split">
        <button class="manager-secondary-action" disabled={busy} onclick={persist}>
          <Icon name="save" /> Save
        </button>
        <button class="manager-primary-action" disabled={busy} onclick={toggle}>
          {runningIds.has(selected.id) ? "Stop Tunnel" : "Start Tunnel"}
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
</section>
