<script lang="ts">
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import Icon from "../../components/Icon.svelte";
  import { confirmDialog } from "../../lib/dialog";
  import { beginMarqueeSelection } from "../../lib/marqueeSelection";
  import { deleteSnippet, listSnippets, saveSnippet } from "../../lib/ipc";
  import type { Snippet } from "../../lib/types";
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

  let snippets = $state<Snippet[]>([]);
  let selected = $state<Snippet | null>(null);
  let snippetContextMenu = $state<{ snippet: Snippet; x: number; y: number } | null>(null);
  let bulkSnippetContextMenu = $state<{ x: number; y: number } | null>(null);
  let selectedSnippetIds = $state<string[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  let message = $state("");
  let isError = $state(false);
  let inspectorMenuOpen = $state(false);
  let collectionView = $state<CollectionView>("grid");
  let collectionSort = $state<CollectionSort>("az");
  let collectionQuery = $state("");
  let editorBaseline = $state<string | null>(null);
  const editorDirty = $derived(Boolean(selected) && draftChanged(editorBaseline, selected));
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
    return () => ondirtychange(false);
  });

  $effect(() => ondirtychange(editorDirty));

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
      editorBaseline = selected ? draftSnapshot(selected) : null;
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      loading = false;
    }
  }

  async function createSnippet() {
    if (!(await confirmDiscardChanges(editorDirty))) return;
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
    editorBaseline = draftSnapshot(selected);
  }

  async function chooseSnippet(snippet: Snippet) {
    if (selected?.id !== snippet.id && !(await confirmDiscardChanges(editorDirty))) return;
    selected = { ...snippet };
    inspectorMenuOpen = false;
    message = "";
    editorBaseline = draftSnapshot(selected);
  }

  async function handleSnippetClick(event: MouseEvent, snippet: Snippet) {
    if (event.ctrlKey || event.metaKey) {
      selectedSnippetIds = selectedSnippetIds.includes(snippet.id)
        ? selectedSnippetIds.filter((id) => id !== snippet.id)
        : [...selectedSnippetIds, snippet.id];
      return;
    }
    selectedSnippetIds = [snippet.id];
    await chooseSnippet(snippet);
  }

  function startSnippetMarquee(event: PointerEvent) {
    beginMarqueeSelection(
      event,
      event.currentTarget as HTMLElement,
      selectedSnippetIds,
      (ids) => (selectedSnippetIds = ids)
    );
  }

  function openSnippetContextMenu(event: MouseEvent, snippet: Snippet) {
    event.preventDefault();
    event.stopPropagation();
    if (selectedSnippetIds.length > 1 && selectedSnippetIds.includes(snippet.id)) {
      snippetContextMenu = null;
      bulkSnippetContextMenu = {
        x: Math.max(10, Math.min(event.clientX, window.innerWidth - 220)),
        y: Math.max(10, Math.min(event.clientY, window.innerHeight - 156))
      };
      return;
    }
    selectedSnippetIds = [snippet.id];
    bulkSnippetContextMenu = null;
    snippetContextMenu = {
      snippet: { ...snippet },
      x: Math.max(10, Math.min(event.clientX, window.innerWidth - 220)),
      y: Math.max(10, Math.min(event.clientY, window.innerHeight - 156))
    };
  }

  function selectedSnippets(): Snippet[] {
    const ids = new Set(selectedSnippetIds);
    return snippets.filter((snippet) => ids.has(snippet.id));
  }

  function takeContextSnippet(): Snippet | null {
    const snippet = snippetContextMenu?.snippet ?? null;
    snippetContextMenu = null;
    return snippet;
  }

  async function closeInspector() {
    if (!(await confirmDiscardChanges(editorDirty))) return;
    selected = null;
    editorBaseline = null;
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

  async function duplicateSnippet(snippet: Snippet) {
    if (selected?.id !== snippet.id && !(await confirmDiscardChanges(editorDirty))) return;
    saving = true;
    try {
      const duplicate = await saveSnippet({
        ...snippet,
        id: crypto.randomUUID(),
        title: `${snippet.title} copy`,
        created_at: new Date().toISOString()
      });
      await refresh(duplicate.id);
      message = "Snippet duplicated";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      saving = false;
    }
  }

  async function removeSnippet(snippet: Snippet) {
    if (!snippets.some((item) => item.id === snippet.id)) return;
    if (
      !(await confirmDialog({
        title: "Remove snippet?",
        message: `Remove “${snippet.title}” from your vault?`,
        confirmLabel: "Remove",
        danger: true
      }))
    ) return;
    try {
      await deleteSnippet(snippet.id);
      if (selected?.id === snippet.id) selected = null;
      await refresh();
      message = "";
    } catch (cause) {
      showError(cause);
    }
  }

  async function duplicateSelectedSnippets() {
    const targets = selectedSnippets();
    bulkSnippetContextMenu = null;
    if (!targets.length || saving) return;
    saving = true;
    try {
      const duplicates = await Promise.all(targets.map((snippet) => saveSnippet({
        ...snippet,
        id: crypto.randomUUID(),
        title: `${snippet.title} copy`,
        created_at: new Date().toISOString()
      })));
      await refresh();
      selectedSnippetIds = duplicates.map((snippet) => snippet.id);
      message = `${duplicates.length} snippets duplicated`;
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      saving = false;
    }
  }

  async function removeSelectedSnippets() {
    const targets = selectedSnippets();
    bulkSnippetContextMenu = null;
    if (!targets.length || !(await confirmDialog({
      title: `Remove ${targets.length} snippets?`,
      message: "Remove every selected snippet from your vault?",
      confirmLabel: "Remove all",
      danger: true
    }))) return;
    try {
      await Promise.all(targets.map((snippet) => deleteSnippet(snippet.id)));
      if (selected && selectedSnippetIds.includes(selected.id)) selected = null;
      selectedSnippetIds = [];
      await refresh();
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

<svelte:window
  onclick={() => {
    snippetContextMenu = null;
    bulkSnippetContextMenu = null;
  }}
  onkeydown={(event) => {
    if (event.key === "Escape") {
      snippetContextMenu = null;
      bulkSnippetContextMenu = null;
    }
  }}
/>

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
    <div
      class="manager-scroll"
      role="region"
      aria-label="Snippet selection area"
      onpointerdown={startSnippetMarquee}
    >
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
        <div
          class="manager-grid selectable-grid"
          role="list"
          class:list-view={collectionView === "list"}
        >
          {#each visibleSnippets as snippet (snippet.id)}
            <button
              class="manager-card"
              class:selected={selectedSnippetIds.includes(snippet.id)}
              data-select-id={snippet.id}
              onclick={(event) => void handleSnippetClick(event, snippet)}
              oncontextmenu={(event) => openSnippetContextMenu(event, snippet)}
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
                  <button onclick={() => selected && void duplicateSnippet(selected)}>
                    <Icon name="copy" /> Duplicate
                  </button>
                  <button class="danger-copy" onclick={() => selected && void removeSnippet(selected)}>
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

  {#if snippetContextMenu}
    <div
      class="host-context-menu manager-context-menu"
      style:left={`${snippetContextMenu.x}px`}
      style:top={`${snippetContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <button
        role="menuitem"
        onclick={() => {
          const snippet = takeContextSnippet();
          if (snippet) void chooseSnippet(snippet);
        }}
      ><Icon name="edit" size={17} /><span>Edit</span></button>
      <button
        role="menuitem"
        onclick={() => {
          const snippet = takeContextSnippet();
          if (snippet) void duplicateSnippet(snippet);
        }}
      ><Icon name="copy" size={17} /><span>Duplicate</span></button>
      <div class="context-separator"></div>
      <button
        class="danger"
        role="menuitem"
        onclick={() => {
          const snippet = takeContextSnippet();
          if (snippet) void removeSnippet(snippet);
        }}
      ><Icon name="trash" size={17} /><span>Remove</span></button>
    </div>
  {/if}

  {#if bulkSnippetContextMenu && selectedSnippetIds.length > 1}
    <div
      class="host-context-menu manager-context-menu"
      style:left={`${bulkSnippetContextMenu.x}px`}
      style:top={`${bulkSnippetContextMenu.y}px`}
      role="menu"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={(event) => event.stopPropagation()}
      oncontextmenu={(event) => event.preventDefault()}
    >
      <div class="bulk-menu-heading">{selectedSnippetIds.length} snippets selected</div>
      <button role="menuitem" onclick={() => void duplicateSelectedSnippets()}>
        <Icon name="copy" size={17} /><span>Duplicate all</span>
      </button>
      <div class="context-separator"></div>
      <button class="danger" role="menuitem" onclick={() => void removeSelectedSnippets()}>
        <Icon name="trash" size={17} /><span>Remove all</span>
      </button>
    </div>
  {/if}
</section>
