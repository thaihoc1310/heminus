<script lang="ts">
  import Icon from "../../components/Icon.svelte";
  import type { SessionProcess } from "../../lib/types";

  let {
    heading,
    message,
    groups,
    busy = false,
    error = "",
    onCancel,
    onCloseAndStop
  }: {
    heading: string;
    message: string;
    groups: Array<{ paneId?: string; title: string; processes: SessionProcess[] }>;
    busy?: boolean;
    error?: string;
    onCancel: () => void;
    onCloseAndStop: () => void;
  } = $props();

  const processes = $derived(groups.flatMap((group) => group.processes));
  const countLabel = $derived(
    `${processes.length} ${processes.length === 1 ? "process" : "processes"}`
  );

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape" || busy) return;
    event.preventDefault();
    onCancel();
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
      <span class="terminal-close-mark"><Icon name="alert" size={20} /></span>
      <div class="terminal-close-heading">
        <h2 id="terminal-close-title">{heading}</h2>
        <p>{countLabel} still running here. {message}</p>
      </div>
    </header>

    <div class="terminal-close-list">
      {#each groups as group (group.paneId ?? group.title)}
        {#if groups.length > 1}
          <div class="terminal-close-group">{group.title}</div>
        {/if}
        {#each group.processes as process (`${group.paneId ?? group.title}-${process.pid}`)}
          <div class="terminal-close-row">
            <span class="terminal-close-dot"></span>
            <span class="terminal-close-detail">
              <strong>{process.name}{#if process.pid > 0}<i>{process.pid}</i>{/if}</strong>
              <small title={process.command}>{process.command}</small>
            </span>
          </div>
        {/each}
      {/each}
    </div>

    {#if error}<p class="terminal-close-error" role="alert">{error}</p>{/if}

    <footer>
      <button class="quiet-button" disabled={busy} onclick={onCancel}>Cancel</button>
      <span class="grow"></span>
      <button class="dialog-primary danger" disabled={busy} onclick={onCloseAndStop}>
        Stop &amp; close
      </button>
    </footer>
  </div>
</div>
