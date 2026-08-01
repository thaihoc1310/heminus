<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import Icon from "../../components/Icon.svelte";
  import HostIcon from "../../components/HostIcon.svelte";
  import { disconnectSession, listHosts, listSessions } from "../../lib/ipc";
  import type { Host, SessionRecord } from "../../lib/types";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  interface SessionGroup {
    key: string;
    title: string;
    latest: SessionRecord;
    active: SessionRecord[];
  }

  let sessions = $state<SessionRecord[]>([]);
  let hosts = $state<Host[]>([]);
  let loading = $state(true);
  let error = $state("");
  let collectionView = $state<CollectionView>("list");
  let collectionSort = $state<CollectionSort>("newest");
  let expanded = $state<Record<string, boolean>>({});
  let disconnecting = $state<Set<string>>(new Set());
  let refreshPromise: Promise<void> | null = null;
  let disposed = false;

  const visibleGroups = $derived.by(() => {
    const grouped = new Map<string, SessionRecord[]>();
    for (const session of sessions) {
      const key = session.host_id ?? "local";
      grouped.set(key, [...(grouped.get(key) ?? []), session]);
    }
    return [...grouped.entries()]
      .map(([key, items]): SessionGroup => {
        const sorted = [...items].sort(
          (left, right) => Date.parse(right.started_at) - Date.parse(left.started_at)
        );
        return {
          key,
          title: hosts.find((host) => host.id === sorted[0]?.host_id)?.label ?? sorted[0]?.title ?? "Terminal",
          latest: sorted[0],
          active: sorted.filter((session) => !session.ended_at && session.status === "connected")
        };
      })
      .sort((left, right) =>
        compareCollectionItems(
          left.title,
          right.title,
          left.latest.started_at,
          right.latest.started_at,
          collectionSort
        )
      );
  });

  onMount(() => {
    disposed = false;
    void listHosts("").then((items) => {
      if (!disposed) hosts = items;
    });
    void refresh();
    const timer = window.setInterval(() => void refresh(true), 1500);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  });

  function refresh(silent = false): Promise<void> {
    if (refreshPromise) return refreshPromise;
    if (!silent) loading = true;
    const current = (async () => {
      try {
        const nextSessions = await listSessions();
        if (disposed) return;
        sessions = nextSessions;
        error = "";
      } catch (cause) {
        if (!disposed) error = cause instanceof Error ? cause.message : String(cause);
      } finally {
        if (!disposed && !silent) loading = false;
      }
    })();
    refreshPromise = current;
    return current.finally(() => {
      if (refreshPromise === current) refreshPromise = null;
    });
  }

  function formatDate(value: string): string {
    return new Intl.DateTimeFormat("en", {
      dateStyle: "medium",
      timeStyle: "short"
    }).format(new Date(value));
  }

  function duration(session: SessionRecord): string {
    if (!session.ended_at) return "Active now";
    const seconds = Math.max(
      0,
      Math.round((Date.parse(session.ended_at) - Date.parse(session.started_at)) / 1000)
    );
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  }

  function setDisconnecting(ids: string[], active: boolean) {
    const next = new Set(disconnecting);
    for (const id of ids) active ? next.add(id) : next.delete(id);
    disconnecting = next;
  }

  async function disconnectSessions(items: SessionRecord[]) {
    const ids = items.map((session) => session.id);
    if (ids.length === 0 || ids.some((id) => disconnecting.has(id))) return;
    setDisconnecting(ids, true);
    try {
      await Promise.all(ids.map((id) => disconnectSession(id)));
      await refresh(true);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      setDisconnecting(ids, false);
    }
  }
</script>

<section class="logs-page">
  <header class="content-toolbar">
    <div class="toolbar-title"><Icon name="clock" /> Recent Connections</div>
    <div class="toolbar-spacer"></div>
    <CollectionControls
      view={collectionView}
      sort={collectionSort}
      onviewchange={(view) => (collectionView = view)}
      onsortchange={(sort) => (collectionSort = sort)}
    />
    <span class="manager-count">Metadata only · terminal output is not recorded</span>
  </header>
  <div class="logs-content">
    <div class="page-heading">
      <div><h1>Logs</h1><p>Active terminals grouped by host, or the latest disconnected session.</p></div>
      <span>{visibleGroups.length} connections</span>
    </div>
    {#if loading}
      <div class="loading-row">Loading session history…</div>
    {:else if error && sessions.length === 0}
      <div class="error-row">{error}</div>
    {:else if sessions.length === 0}
      <div class="empty-page manager-empty">
        <div class="empty-glyph"><Icon name="clock" size={30} /></div>
        <h2>No sessions yet</h2>
        <p>Open a local terminal or connect to a host to start your history.</p>
      </div>
    {:else}
      {#if error}<div class="error-row compact-error">{error}</div>{/if}
      <div class="session-list grouped-sessions" class:grid-view={collectionView === "grid"}>
        {#if collectionView === "list"}
          <div class="session-header"><span>Started</span><span>Connection</span><span>Duration</span><span>Status</span></div>
        {/if}
        {#each visibleGroups as group (group.key)}
          <article class="session-group" class:expanded={expanded[group.key]}>
            <div class="session-row session-group-row">
              <time datetime={group.latest.started_at}>{formatDate(group.latest.started_at)}</time>
              <span class="session-title">
                {#if group.active.length > 0}
                  <button
                    class="session-disclosure"
                    class:expanded={expanded[group.key]}
                    title={expanded[group.key] ? "Hide active terminals" : "Show active terminals"}
                    aria-expanded={Boolean(expanded[group.key])}
                    onclick={() => (expanded[group.key] = !expanded[group.key])}
                  ><Icon name="chevron-right" size={15} /></button>
                {:else}
                  <span class="session-disclosure-placeholder"></span>
                {/if}
                <span class="host-badge mini {group.latest.host_id ? 'amber' : 'blue'}">
                  <HostIcon hostId={group.latest.host_id} size={15} />
                </span>
                <strong>{group.title}</strong>
                {#if group.active.length > 1}<small>{group.active.length} tabs</small>{/if}
              </span>
              <span>{group.active.length > 0 ? `${group.active.length} active` : duration(group.latest)}</span>
              {#if group.active.length > 0}
                <button
                  class="status-pill connected disconnect-status"
                  disabled={group.active.some((session) => disconnecting.has(session.id))}
                  onclick={() => void disconnectSessions(group.active)}
                >
                  <span class="status-default">active</span>
                  <span class="status-hover">disconnect all</span>
                </button>
              {:else}
                <span class="status-pill {group.latest.status}">{group.latest.status}</span>
              {/if}
            </div>
            {#if group.active.length > 0 && expanded[group.key]}
              <div class="session-children">
                {#each group.active as session, index (session.id)}
                  <div class="session-child-row">
                    <time datetime={session.started_at}>{formatDate(session.started_at)}</time>
                    <span class="session-child-title"><HostIcon hostId={session.host_id} size={14} /><strong>{session.title}</strong><small>Tab {index + 1}</small></span>
                    <span>{duration(session)}</span>
                    <button
                      class="session-disconnect-button"
                      disabled={disconnecting.has(session.id)}
                      onclick={() => void disconnectSessions([session])}
                    >{disconnecting.has(session.id) ? "Disconnecting…" : "Disconnect"}</button>
                  </div>
                {/each}
              </div>
            {/if}
          </article>
        {/each}
      </div>
    {/if}
  </div>
</section>
