<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import Icon from "../../components/Icon.svelte";
  import { confirmDialog } from "../../lib/dialog";
  import { deleteSnippet, listSnippets, saveSnippet } from "../../lib/ipc";
  import type { Snippet } from "../../lib/types";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  let snippets = $state<Snippet[]>([]);
  let selected = $state<Snippet | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let message = $state("");
  let isError = $state(false);
  let inspectorMenuOpen = $state(false);
  let collectionView = $state<CollectionView>("grid");
  let collectionSort = $state<CollectionSort>("az");
  let collectionQuery = $state("");
  const visibleSnippets = $derived.by(() =>
    snippets
      .filter((snippet) =>
        `${snippet.title} ${snippet.command} ${snippet.description}`
          .toLowerCase()
          .includes(collectionQuery.trim().toLowerCase())
      )
      .sort((left, right) =>
        compareCollectionItems(
          left.title,
          right.title,
          left.created_at,
          right.created_at,
          collectionSort
        )
      )
  );

  onMount(() => {
    void refresh();
  });

  async function refresh(preferredId?: string) {
    loading = true;
    try {
      snippets = await listSnippets();
      const next = preferredId
        ? snippets.find((snippet) => snippet.id === preferredId) ?? null
        : selected
          ? snippets.find((snippet) => snippet.id === selected?.id) ?? null
          : null;
      selected = next ? { ...next } : null;
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      loading = false;
    }
  }

  function createSnippet() {
    selected = {
      id: crypto.randomUUID(),
      title: "",
      command: "",
      description: "",
      favorite: false,
      created_at: new Date().toISOString()
    };
    inspectorMenuOpen = false;
    message = "Snippets are stored only in your local vault.";
    isError = false;
  }

  function chooseSnippet(snippet: Snippet) {
    selected = { ...snippet };
    inspectorMenuOpen = false;
    message = "";
  }

  function closeInspector() {
    selected = null;
    inspectorMenuOpen = false;
    message = "";
  }

  async function persist() {
    if (!selected || saving) return;
    saving = true;
    try {
      const saved = await saveSnippet(selected);
      await refresh(saved.id);
      message = "Snippet saved";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!selected || !snippets.some((item) => item.id === selected?.id)) return;
    if (
      !(await confirmDialog({
        title: "Remove snippet?",
        message: `Remove “${selected.title}” from your vault?`,
        confirmLabel: "Remove",
        danger: true
      }))
    ) return;
    try {
      await deleteSnippet(selected.id);
      await refresh();
      message = "";
    } catch (cause) {
      showError(cause);
    }
  }

  async function copyCommand() {
    if (!selected?.command) return;
    await navigator.clipboard.writeText(selected.command);
    message = "Command copied";
    isError = false;
  }

  function showError(cause: unknown) {
    message = cause instanceof Error ? cause.message : String(cause);
    isError = true;
  }
</script>

<section class="manager-page" class:with-inspector={Boolean(selected)}>
  <div class="manager-main">
    <header class="content-toolbar">
      <button class="secondary-action" onclick={createSnippet}><Icon name="plus" /> New snippet</button>
      <div class="toolbar-spacer"></div>
      <CollectionControls
        view={collectionView}
        sort={collectionSort}
        onviewchange={(view) => (collectionView = view)}
        onsortchange={(sort) => (collectionSort = sort)}
        searchValue={collectionQuery}
        searchPlaceholder="Search snippets"
        onsearchchange={(value) => (collectionQuery = value)}
      />
      <span class="manager-count">{snippets.length} saved</span>
    </header>
    <div class="manager-scroll">
      <div class="page-heading">
        <div><h1>Snippets</h1><p>Keep commands you use often within easy reach.</p></div>
      </div>
      {#if loading}
        <div class="loading-row">Loading snippets…</div>
      {:else if snippets.length === 0}
        <div class="empty-page manager-empty">
          <div class="empty-glyph"><Icon name="code" size={30} /></div>
          <h2>Create your first snippet</h2>
          <p>Save a command once, then copy it whenever you need it.</p>
          <button class="secondary-action" onclick={createSnippet}>New snippet</button>
        </div>
      {:else if visibleSnippets.length === 0}
        <div class="empty-page manager-empty">
          <div class="empty-glyph"><Icon name="search" size={28} /></div>
          <h2>No matching snippets</h2>
          <p>Try another name, command, or description.</p>
        </div>
      {:else}
        <div class="manager-grid" class:list-view={collectionView === "list"}>
          {#each visibleSnippets as snippet (snippet.id)}
            <button
              class="manager-card"
              class:selected={selected?.id === snippet.id}
              onclick={() => chooseSnippet(snippet)}
            >
              <span class="manager-icon"><Icon name="code" /></span>
              <span><strong>{snippet.title}</strong><code>{snippet.command}</code></span>
              {#if snippet.favorite}<span class="favorite-dot" title="Favorite"></span>{/if}
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
        <strong>{selected?.title || "New Snippet"}</strong><span>Personal vault</span>
      </div>
      <div class="manager-inspector-header-actions">
        {#if selected}
          <div class="host-editor-menu-anchor">
            <button
              class="icon-button"
              class:active={inspectorMenuOpen}
              title="Snippet actions"
              onclick={() => (inspectorMenuOpen = !inspectorMenuOpen)}
            ><Icon name="more" /></button>
            {#if inspectorMenuOpen}
              <div class="host-editor-menu">
                <button disabled={!selected.command} onclick={() => void copyCommand()}>
                  <Icon name="copy" /> Copy command
                </button>
                {#if snippets.some((snippet) => snippet.id === selected?.id)}
                  <button class="danger-copy" onclick={() => void remove()}>
                    <Icon name="trash" /> Remove
                  </button>
                {/if}
              </div>
            {/if}
          </div>
          <button class="icon-button" title="Close editor" onclick={closeInspector}>
            <Icon name="close" />
          </button>
        {/if}
      </div>
    </header>
    {#if selected}
      <div class="inspector-scroll">
        <section class="form-card">
          <h2>Command</h2>
          <label for="snippet-title">Name</label>
          <input id="snippet-title" bind:value={selected.title} placeholder="Check disk usage" />
          <label for="snippet-command">Shell command</label>
          <textarea id="snippet-command" bind:value={selected.command} placeholder="df -h"></textarea>
          <label for="snippet-description">Description</label>
          <textarea
            id="snippet-description"
            class="short"
            bind:value={selected.description}
            placeholder="Optional note"
          ></textarea>
          <label class="check-row">
            <input type="checkbox" bind:checked={selected.favorite} />
            <span>Mark as favorite</span>
          </label>
        </section>
        {#if message}<p class:error-message={isError} class="form-message">{message}</p>{/if}
      </div>
      <footer class="manager-inspector-footer">
        <button class="manager-primary-action" disabled={saving} onclick={persist}>
          <Icon name="save" /> {saving ? "Saving…" : "Save"}
        </button>
      </footer>
    {:else}
      <div class="empty-inspector">
        <Icon name="code" size={32} />
        <p>Select a snippet or create a new one.</p>
      </div>
    {/if}
  </aside>
  {/if}
</section>
