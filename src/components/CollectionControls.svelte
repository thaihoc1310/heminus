<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { CollectionSort, CollectionView } from "../lib/collection";

  let {
    view,
    sort,
    onviewchange,
    onsortchange,
    searchValue = "",
    searchPlaceholder = "Search",
    onsearchchange
  }: {
    view: CollectionView;
    sort: CollectionSort;
    onviewchange: (view: CollectionView) => void;
    onsortchange: (sort: CollectionSort) => void;
    searchValue?: string;
    searchPlaceholder?: string;
    onsearchchange?: (value: string) => void;
  } = $props();

  let openMenu = $state<"view" | "sort" | null>(null);
  let searchOpen = $state(false);
  let searchInput = $state<HTMLInputElement>();

  $effect(() => {
    if (searchValue) searchOpen = true;
  });

  function chooseView(next: CollectionView) {
    onviewchange(next);
    openMenu = null;
  }

  function chooseSort(next: CollectionSort) {
    onsortchange(next);
    openMenu = null;
  }

  function openSearch() {
    searchOpen = true;
    requestAnimationFrame(() => searchInput?.focus());
  }

  function closeSearch() {
    if (!searchValue.trim()) searchOpen = false;
  }
</script>

<svelte:window onclick={() => (openMenu = null)} />

<div class="collection-controls">
  {#if onsearchchange}
    <div class="collection-search" class:open={searchOpen || Boolean(searchValue)}>
      {#if searchOpen || searchValue}
        <Icon name="search" size={17} />
        <input
          bind:this={searchInput}
          value={searchValue}
          aria-label={searchPlaceholder}
          placeholder={searchPlaceholder}
          oninput={(event) => onsearchchange?.(event.currentTarget.value)}
          onblur={closeSearch}
        />
        {#if searchValue}
          <button
            class="search-clear"
            title="Clear search"
            aria-label="Clear search"
            onclick={() => {
              onsearchchange?.("");
              searchOpen = false;
            }}
          ><Icon name="close" size={13} /></button>
        {/if}
      {:else}
        <button
          class="icon-button"
          title="Search"
          aria-label="Search"
          onclick={openSearch}
        ><Icon name="search" /></button>
      {/if}
    </div>
  {/if}
  <div class="collection-control-anchor">
    <button
      class="icon-button"
      class:active={openMenu === "view"}
      title="Change layout"
      aria-label="Change layout"
      aria-expanded={openMenu === "view"}
      onclick={(event) => {
        event.stopPropagation();
        openMenu = openMenu === "view" ? null : "view";
      }}
    ><Icon name={view === "grid" ? "grid" : "list"} /></button>
    {#if openMenu === "view"}
      <div class="collection-menu view-menu">
        <button onclick={() => chooseView("grid")}>
          <Icon name="grid" /><span>Grid</span>{#if view === "grid"}<Icon name="check" />{/if}
        </button>
        <button onclick={() => chooseView("list")}>
          <Icon name="list" /><span>List</span>{#if view === "list"}<Icon name="check" />{/if}
        </button>
      </div>
    {/if}
  </div>

  <div class="collection-control-anchor">
    <button
      class="icon-button"
      class:active={openMenu === "sort"}
      title="Sort items"
      aria-label="Sort items"
      aria-expanded={openMenu === "sort"}
      onclick={(event) => {
        event.stopPropagation();
        openMenu = openMenu === "sort" ? null : "sort";
      }}
    ><Icon name="sort" /></button>
    {#if openMenu === "sort"}
      <div class="collection-menu sort-menu">
        <button onclick={() => chooseSort("az")}>
          <Icon name="sort-az" /><span>A–Z</span>{#if sort === "az"}<Icon name="check" />{/if}
        </button>
        <button onclick={() => chooseSort("za")}>
          <Icon name="sort-za" /><span>Z–A</span>{#if sort === "za"}<Icon name="check" />{/if}
        </button>
        <div class="collection-menu-separator"></div>
        <button onclick={() => chooseSort("newest")}>
          <Icon name="calendar" /><span>Newest to oldest</span>{#if sort === "newest"}<Icon name="check" />{/if}
        </button>
        <button onclick={() => chooseSort("oldest")}>
          <Icon name="calendar" /><span>Oldest to newest</span>{#if sort === "oldest"}<Icon name="check" />{/if}
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .collection-controls {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .collection-control-anchor {
    position: relative;
  }

  .collection-search {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    min-width: 30px;
    height: 30px;
    color: #81939c;
    border-radius: 9px;
    transition:
      width 140ms ease,
      background 140ms ease;
  }

  .collection-search.open {
    width: 168px;
    padding: 0 7px 0 9px;
    background: #edf2f4;
  }

  .collection-search > input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 7px;
    color: #172033;
    border: 0;
    outline: 0;
    background: transparent;
    font-size: 12px;
  }

  .collection-search > input:focus,
  .collection-search > input:focus-visible {
    outline: 0;
    box-shadow: none;
  }

  .search-clear {
    display: grid;
    place-items: center;
    flex: 0 0 22px;
    width: 22px;
    height: 22px;
    padding: 0;
    color: #84939a;
    border-radius: 6px;
    cursor: pointer;
  }

  .search-clear:hover {
    background: #dfe6e9;
  }

  .collection-control-anchor > button.active {
    background: #e1e7e9;
  }

  .collection-menu {
    position: absolute;
    z-index: 70;
    top: 38px;
    right: 0;
    display: grid;
    width: 206px;
    padding: 7px;
    border: 1px solid #dfe6e9;
    border-radius: 14px;
    background: #f9fbfc;
    box-shadow: 0 18px 44px #17212a2b;
  }

  .view-menu {
    width: 154px;
  }

  .collection-menu button {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) 18px;
    align-items: center;
    gap: 9px;
    min-height: 40px;
    padding: 0 10px;
    color: #172033;
    text-align: left;
    border-radius: 9px;
    cursor: pointer;
  }

  .collection-menu button:hover {
    background: #e8edef;
  }

  .collection-menu button > :global(svg) {
    color: #81949d;
  }

  .collection-menu button > :global(svg:last-child) {
    color: #6d8792;
  }

  .collection-menu-separator {
    height: 1px;
    margin: 5px 9px;
    background: #e0e6e8;
  }
</style>
