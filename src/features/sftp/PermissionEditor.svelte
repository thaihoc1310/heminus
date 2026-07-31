<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "../../components/Icon.svelte";
  import { confirmDiscardChanges } from "../../lib/unsavedChanges";

  let {
    path,
    initialMode,
    owner,
    group,
    onclose,
    onsave
  }: {
    path: string;
    initialMode: number;
    owner: string | null;
    group: string | null;
    onclose: () => void;
    onsave: (mode: number) => Promise<void>;
  } = $props();

  const accessRows = [
    { label: "Owner", bits: [0o400, 0o200, 0o100] },
    { label: "Group", bits: [0o040, 0o020, 0o010] },
    { label: "Others", bits: [0o004, 0o002, 0o001] }
  ];

  const baselineMode = untrack(() => initialMode & 0o7777);
  let mode = $state(baselineMode);
  let saving = $state(false);
  let saveError = $state("");

  function toggle(bit: number) {
    mode ^= bit;
  }

  function modeLabel(): string {
    return (mode & 0o7777).toString(8).padStart(3, "0");
  }

  async function save() {
    if (saving) return;
    saving = true;
    saveError = "";
    try {
      await onsave(mode);
    } catch (cause) {
      saveError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      saving = false;
    }
  }

  async function requestClose() {
    if (saving || !(await confirmDiscardChanges(mode !== baselineMode))) return;
    onclose();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !saving) void requestClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="permission-editor-layer">
  <button
    class="permission-editor-backdrop"
    aria-label="Close permission editor"
    disabled={saving}
    onclick={() => void requestClose()}
  ></button>
  <div
    class="permission-editor"
    role="dialog"
    aria-modal="true"
    aria-labelledby="permission-editor-title"
  >
    <header>
      <div>
        <h2 id="permission-editor-title">Edit permissions</h2>
        <span class="permission-mode">{modeLabel()}</span>
      </div>
      <button class="permission-close" title="Close" disabled={saving} onclick={() => void requestClose()}>
        <Icon name="close" size={19} />
      </button>
    </header>

    <div class="permission-editor-body">
      <p class="permission-path" title={path}>{path}</p>

      <section class="permission-access">
        <div class="permission-access-header">
          <strong>File access</strong>
          <span>Read</span>
          <span>Write</span>
          <span>Execute</span>
        </div>
        {#each accessRows as row (row.label)}
          <div class="permission-access-row">
            <strong>{row.label}</strong>
            {#each row.bits as bit (`${row.label}-${bit}`)}
              <button
                type="button"
                class="permission-switch"
                class:active={(mode & bit) !== 0}
                role="switch"
                aria-checked={(mode & bit) !== 0}
                disabled={saving}
                aria-label={`${row.label} ${bit === row.bits[0] ? "read" : bit === row.bits[1] ? "write" : "execute"}`}
                onclick={() => toggle(bit)}
              ><i></i></button>
            {/each}
          </div>
        {/each}
      </section>

      <section class="permission-ownership">
        <h3>Ownership</h3>
        <div><span>User</span><strong>{owner ?? "Unavailable"}</strong></div>
        <div><span>Group</span><strong>{group ?? "Unavailable"}</strong></div>
      </section>

      {#if saveError}<p class="permission-error">{saveError}</p>{/if}
    </div>

    <footer>
      <button class="quiet-button" disabled={saving} onclick={() => void requestClose()}>Cancel</button>
      <button class="dialog-primary" disabled={saving} onclick={() => void save()}>
        {saving ? "Saving…" : "Save"}
      </button>
    </footer>
  </div>
</div>
