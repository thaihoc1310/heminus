<script lang="ts">
  import Icon from "../../components/Icon.svelte";
  import type { SessionProcess } from "../../lib/types";

  let {
    title,
    processes,
    busy = false,
    error = "",
    onCancel,
    onCloseAndStop,
    onCloseAndKeep,
    onStopSelected
  }: {
    title: string;
    processes: SessionProcess[];
    busy?: boolean;
    error?: string;
    onCancel: () => void;
    onCloseAndStop: () => void;
    onCloseAndKeep: () => void;
    onStopSelected: (pids: number[]) => void;
  } = $props();

  let reviewing = $state(false);
  let selected = $state<number[]>([]);

  const allSelected = $derived(
    processes.length > 0 && selected.length === processes.length
  );
  const countLabel = $derived(
    `${processes.length} ${processes.length === 1 ? "process" : "processes"}`
  );

  function toggle(pid: number) {
    selected = selected.includes(pid)
      ? selected.filter((value) => value !== pid)
      : [...selected, pid];
  }

  function toggleAll() {
    selected = allSelected ? [] : processes.map((process) => process.pid);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape" || busy) return;
    event.preventDefault();
    if (reviewing) reviewing = false;
    else onCancel();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app-dialog-layer">
  <button class="app-dialog-backdrop" aria-label="Keep the terminal open" onclick={onCancel}></button>
  <div
    class="app-dialog terminal-close-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="terminal-close-title"
  >
    <header class="terminal-close-header">
      {#if reviewing}
        <button
          class="terminal-close-back"
          title="Back"
          aria-label="Back"
          onclick={() => (reviewing = false)}
        ><Icon name="arrow-left" size={18} /></button>
      {:else}
        <span class="terminal-close-mark"><Icon name="alert" size={20} /></span>
      {/if}
      <div class="terminal-close-heading">
        <h2 id="terminal-close-title">
          {reviewing ? "Choose what to stop" : `Close “${title}”?`}
        </h2>
        <p>
          {reviewing
            ? "Whatever you leave unticked keeps running after the terminal closes."
            : `${countLabel} still running here. Closing the terminal stops them.`}
        </p>
      </div>
    </header>

    <div class="terminal-close-list" class:selectable={reviewing}>
      {#if reviewing}
        <label class="terminal-close-row terminal-close-selectall">
          <input type="checkbox" checked={allSelected} onchange={toggleAll} />
          <span class="terminal-close-box" aria-hidden="true"><Icon name="check" size={12} strokeWidth={3} /></span>
          <span>Select all</span>
          <span class="terminal-close-count">{selected.length}/{processes.length}</span>
        </label>
      {/if}
      {#each processes as process (process.pid)}
        {#if reviewing}
          <label class="terminal-close-row" class:picked={selected.includes(process.pid)}>
            <input
              type="checkbox"
              checked={selected.includes(process.pid)}
              onchange={() => toggle(process.pid)}
            />
            <span class="terminal-close-box" aria-hidden="true"><Icon name="check" size={12} strokeWidth={3} /></span>
            <span class="terminal-close-detail">
              <strong>{process.name}<i>{process.pid}</i></strong>
              <small title={process.command}>{process.command}</small>
            </span>
          </label>
        {:else}
          <div class="terminal-close-row">
            <span class="terminal-close-dot"></span>
            <span class="terminal-close-detail">
              <strong>{process.name}<i>{process.pid}</i></strong>
              <small title={process.command}>{process.command}</small>
            </span>
          </div>
        {/if}
      {/each}
    </div>

    {#if error}<p class="terminal-close-error" role="alert">{error}</p>{/if}

    <footer>
      {#if reviewing}
        <!--
          One outcome only: close the terminal, stopping exactly what is ticked.
          A separate "stop selected" button read as a rival to "close terminal"
          and silently threw the ticks away.
        -->
        <button class="quiet-button" disabled={busy} onclick={() => (reviewing = false)}>
          Back
        </button>
        <span class="grow"></span>
        <span class="terminal-close-outcome">
          {selected.length === 0
            ? `All ${processes.length} keep running`
            : `${processes.length - selected.length} keep running`}
        </span>
        <button
          class="dialog-primary"
          class:danger={selected.length > 0}
          disabled={busy}
          onclick={() =>
            selected.length === 0 ? onCloseAndKeep() : onStopSelected(selected)}
        >{selected.length === 0
          ? "Close terminal"
          : `Stop ${selected.length} & close`}</button>
      {:else}
        <button class="quiet-button" disabled={busy} onclick={onCancel}>Cancel</button>
        <span class="grow"></span>
        <button class="quiet-button" disabled={busy} onclick={() => (reviewing = true)}>
          Keep some running…
        </button>
        <button class="dialog-primary danger" disabled={busy} onclick={onCloseAndStop}>
          Stop all &amp; close
        </button>
      {/if}
    </footer>
  </div>
</div>
