<script lang="ts">
  import { onMount } from "svelte";
  import {
    registerDialogListener,
    type AppDialogRequest
  } from "../lib/dialog";

  let request = $state<AppDialogRequest | null>(null);
  let value = $state("");
  let inputElement = $state<HTMLInputElement>();
  const pendingRequests: AppDialogRequest[] = [];

  function showNextRequest() {
    if (request || pendingRequests.length === 0) return;
    const next = pendingRequests.shift() ?? null;
    request = next;
    value = next?.kind === "prompt" ? (next.initialValue ?? "") : "";
    requestAnimationFrame(() => inputElement?.focus());
  }

  function resolveRequest(target: AppDialogRequest, accepted: boolean) {
    if (target.kind === "prompt") target.resolve(accepted ? value : null);
    else if (target.kind === "confirm") target.resolve(accepted);
    else target.resolve();
  }

  onMount(() => {
    const unregister = registerDialogListener((next) => {
      pendingRequests.push(next);
      showNextRequest();
    });
    return () => {
      unregister();
      if (request) resolveRequest(request, false);
      request = null;
      for (const pending of pendingRequests.splice(0)) resolveRequest(pending, false);
    };
  });

  function cancel() {
    if (!request) return;
    const current = request;
    request = null;
    resolveRequest(current, false);
    showNextRequest();
  }

  function accept() {
    if (!request) return;
    const current = request;
    request = null;
    resolveRequest(current, true);
    showNextRequest();
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
          {#if request.kind !== "alert"}
            <button type="button" class="quiet-button" onclick={cancel}>Cancel</button>
          {/if}
          <button
            type="submit"
            class:danger={request.kind === "confirm" && request.danger}
            class="dialog-primary"
          >{request.confirmLabel ?? (request.kind === "prompt" ? "Save" : request.kind === "alert" ? "OK" : "Confirm")}</button>
        </footer>
      </form>
    </div>
  </div>
{/if}
