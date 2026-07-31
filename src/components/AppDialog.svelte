<script lang="ts">
  import { onMount } from "svelte";
  import {
    registerDialogListener,
    type AppDialogRequest
  } from "../lib/dialog";

  let request = $state<AppDialogRequest | null>(null);
  let value = $state("");
  let inputElement = $state<HTMLInputElement>();

  onMount(() =>
    registerDialogListener((next) => {
      request = next;
      value = next.kind === "prompt" ? (next.initialValue ?? "") : "";
      requestAnimationFrame(() => inputElement?.focus());
    })
  );

  function cancel() {
    if (!request) return;
    if (request.kind === "prompt") request.resolve(null);
    else request.resolve(false);
    request = null;
  }

  function accept() {
    if (!request) return;
    if (request.kind === "prompt") request.resolve(value);
    else request.resolve(true);
    request = null;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!request) return;
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if request}
  <div class="app-dialog-layer">
    <button class="app-dialog-backdrop" aria-label="Close dialog" onclick={cancel}></button>
    <div
      class="app-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="app-dialog-title"
    >
      <form
        class="app-dialog-form"
        onsubmit={(event) => {
          event.preventDefault();
          accept();
        }}
      >
        <header>
          <h2 id="app-dialog-title">{request.title}</h2>
          {#if request.message}<p>{request.message}</p>{/if}
        </header>
        {#if request.kind === "prompt"}
          <label for="app-dialog-input">{request.label ?? request.title}</label>
          <input
            id="app-dialog-input"
            bind:this={inputElement}
            bind:value
            placeholder={request.placeholder ?? ""}
            autocomplete="off"
            spellcheck="false"
          />
        {/if}
        <footer>
          <button type="button" class="quiet-button" onclick={cancel}>Cancel</button>
          <button
            type="submit"
            class:danger={request.kind === "confirm" && request.danger}
            class="dialog-primary"
          >{request.confirmLabel ?? (request.kind === "prompt" ? "Save" : "Confirm")}</button>
        </footer>
      </form>
    </div>
  </div>
{/if}
