<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import Icon from "../../components/Icon.svelte";
  import { confirmDialog } from "../../lib/dialog";
  import { deleteKnownHostEntries, listKnownHosts } from "../../lib/ipc";
  import type { KnownHostEntry } from "../../lib/types";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  let entries = $state<KnownHostEntry[]>([]);
  let query = $state("");
  let loading = $state(true);
  let error = $state("");
  let showUnmatched = $state(false);
  let expandedId = $state<string | null>(null);
  let removingId = $state<string | null>(null);
  let message = $state("");
  let collectionView = $state<CollectionView>("grid");
  let collectionSort = $state<CollectionSort>("az");

  interface KnownHostGroup {
    id: string;
    hosts: string;
    entries: KnownHostEntry[];
    unresolved: boolean;
  }

  const unmatchedCount = $derived(
    entries.filter((entry) => entry.hashed && !entry.resolved).length
  );
  const filtered = $derived.by(() => {
    const normalizedQuery = query.trim().toLowerCase();
    const groups = new Map<string, KnownHostGroup>();
    for (const entry of entries) {
      const unresolved = entry.hashed && !entry.resolved;
      if (unresolved && !showUnmatched) continue;
      if (
        normalizedQuery &&
        !`${entry.hosts} ${entry.keyType} ${entry.fingerprint}`
          .toLowerCase()
          .includes(normalizedQuery)
      ) {
        continue;
      }
      const id = unresolved ? `unmatched-${entry.lineNumber}` : entry.hosts;
      const group = groups.get(id);
      if (group) {
        group.entries.push(entry);
      } else {
        groups.set(id, {
          id,
          hosts: entry.hosts,
          entries: [entry],
          unresolved
        });
      }
    }
    return [...groups.values()].sort((left, right) =>
      compareCollectionItems(
        left.hosts,
        right.hosts,
        Math.max(...left.entries.map((entry) => entry.lineNumber)),
        Math.max(...right.entries.map((entry) => entry.lineNumber)),
        collectionSort
      )
    );
  });

  function friendlyKeyType(keyType: string): string {
    return keyType.replace("ssh-", "").replace("ecdsa-sha2-", "").toUpperCase();
  }

  async function refresh() {
    loading = true;
    try {
      entries = await listKnownHosts();
      error = "";
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading = false;
    }
  }

  async function removeGroup(group: KnownHostGroup) {
    if (
      !(await confirmDialog({
        title: "Forget known host?",
        message: `Forget ${group.hosts} and its ${group.entries.length} trusted fingerprint${group.entries.length === 1 ? "" : "s"}?`,
        confirmLabel: "Forget",
        danger: true
      }))
    ) {
      return;
    }
    removingId = group.id;
    try {
      const removed = await deleteKnownHostEntries(
        group.entries.map((entry) => entry.lineNumber)
      );
      if (expandedId === group.id) expandedId = null;
      await refresh();
      message = `${removed} fingerprint${removed === 1 ? "" : "s"} removed from the Heminus vault`;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      removingId = null;
    }
  }

  onMount(refresh);
</script>

<section class="known-hosts-page">
  <header class="content-toolbar">
    <div class="toolbar-title"><Icon name="shield" /> Known Hosts</div>
    <div class="toolbar-spacer"></div>
    <CollectionControls
      view={collectionView}
      sort={collectionSort}
      onviewchange={(view) => (collectionView = view)}
      onsortchange={(sort) => (collectionSort = sort)}
      searchValue={query}
      searchPlaceholder="Search known hosts"
      onsearchchange={(value) => (query = value)}
    />
    <span class="manager-count">Heminus vault</span>
  </header>
  <div class="known-hosts-content">
    <div class="page-heading">
      <div><h1>Known Hosts</h1><p>Server fingerprints trusted only by Heminus.</p></div>
      <span>{filtered.length} host{filtered.length === 1 ? "" : "s"}</span>
    </div>
    {#if unmatchedCount > 0}
      <button class="unmatched-host-toggle" onclick={() => (showUnmatched = !showUnmatched)}>
        <Icon name={showUnmatched ? "close" : "shield"} size={15} />
        {showUnmatched
          ? "Hide unmatched fingerprints"
          : `${unmatchedCount} unmatched vault fingerprint${unmatchedCount === 1 ? "" : "s"} hidden`}
      </button>
    {/if}
    {#if loading}
      <div class="loading-row">Reading known hosts…</div>
    {:else if error}
      <div class="error-row">{error}</div>
    {:else if filtered.length === 0}
      <div class="empty-page manager-empty">
        <div class="empty-glyph"><Icon name="shield" size={30} /></div>
        <h2>No matching host keys</h2>
        <p>New SSH fingerprints appear here after Heminus verifies them.</p>
      </div>
    {:else}
      <div class="known-host-grid" class:list-view={collectionView === "list"}>
        {#each filtered as group (group.id)}
          <article class="known-host-card" class:expanded={expandedId === group.id}>
            <button
              class="known-host-card-main"
              title="Show trusted fingerprints"
              onclick={() => (expandedId = expandedId === group.id ? null : group.id)}
            >
              <span class="manager-icon known-host-icon"><Icon name="fingerprint" /></span>
              <span class="known-host-copy">
                <strong class:unresolved-host={group.unresolved}>{group.hosts}</strong>
                <small>
                  {[...new Set(group.entries.map((entry) => friendlyKeyType(entry.keyType)))].join(" · ")}
                  · {group.entries.length} trusted
                </small>
              </span>
            </button>
            <button
              class="known-host-remove"
              disabled={removingId === group.id}
              title={`Forget ${group.hosts}`}
              onclick={() => void removeGroup(group)}
            ><Icon name="trash" size={16} /></button>
            {#if expandedId === group.id}
              <div class="known-host-fingerprints">
                {#each group.entries as entry (`${entry.lineNumber}-${entry.fingerprint}`)}
                  <div>
                    <span>{friendlyKeyType(entry.keyType)}</span>
                    <code>{entry.fingerprint}</code>
                  </div>
                {/each}
              </div>
            {/if}
          </article>
        {/each}
      </div>
      {#if message}<p class="manager-message">{message}</p>{/if}
    {/if}
  </div>
</section>
