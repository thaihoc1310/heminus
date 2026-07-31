<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import Icon from "../../components/Icon.svelte";
  import { listSessions } from "../../lib/ipc";
  import type { SessionRecord } from "../../lib/types";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  let sessions = $state<SessionRecord[]>([]);
  let loading = $state(true);
  let error = $state("");
  let collectionView = $state<CollectionView>("list");
  let collectionSort = $state<CollectionSort>("newest");
  const visibleSessions = $derived.by(() =>
    [...sessions].sort((left, right) =>
      compareCollectionItems(
        left.title,
        right.title,
        left.started_at,
        right.started_at,
        collectionSort
      )
    )
  );

  onMount(async () => {
    try {
      sessions = await listSessions();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading = false;
    }
  });

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
      <div><h1>Logs</h1><p>The latest local or SSH connection for each host.</p></div>
      <span>{sessions.length} latest</span>
    </div>
    {#if loading}
      <div class="loading-row">Loading session history…</div>
    {:else if error}
      <div class="error-row">{error}</div>
    {:else if sessions.length === 0}
      <div class="empty-page manager-empty">
        <div class="empty-glyph"><Icon name="clock" size={30} /></div>
        <h2>No sessions yet</h2>
        <p>Open a local terminal or connect to a host to start your history.</p>
      </div>
    {:else}
      <div class="session-list" class:grid-view={collectionView === "grid"}>
        {#if collectionView === "list"}
          <div class="session-header"><span>Started</span><span>Connection</span><span>Duration</span><span>Status</span></div>
        {/if}
        {#each visibleSessions as session (session.id)}
          <article class="session-row">
            <time datetime={session.started_at}>{formatDate(session.started_at)}</time>
            <span class="session-title">
              <span class="host-badge mini {session.host_id ? 'amber' : 'blue'}">
                <Icon name="terminal" size={15} />
              </span>
              <strong>{session.title}</strong>
            </span>
            <span>{duration(session)}</span>
            <span class="status-pill {session.status}">{session.status}</span>
          </article>
        {/each}
      </div>
    {/if}
  </div>
</section>
