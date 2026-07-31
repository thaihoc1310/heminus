<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import CollectionControls from "../../components/CollectionControls.svelte";
  import Icon from "../../components/Icon.svelte";
  import { confirmDialog } from "../../lib/dialog";
  import {
    deleteIdentity,
    deleteIdentitySecret,
    generateSshKey,
    importPrivateKey,
    listIdentities,
    readKeyMaterial,
    saveIdentity,
    setIdentitySecret
  } from "../../lib/ipc";
  import type { Identity, IdentityKind, KeyMaterial } from "../../lib/types";
  import {
    compareCollectionItems,
    type CollectionSort,
    type CollectionView
  } from "../../lib/collection";

  let identities = $state<Identity[]>([]);
  let selected = $state<Identity | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let message = $state("");
  let isError = $state(false);
  let pendingSecret = $state("");
  let generationOpen = $state(false);
  let importOpen = $state(false);
  let newKeyMenuOpen = $state(false);
  let importPath = $state("");
  let importLabel = $state("");
  let importPassphrase = $state("");
  let generatedLabel = $state("");
  let generatedPassphrase = $state("");
  let dragActive = $state(false);
  let importing = $state(false);
  let importStatus = $state("");
  let importError = $state(false);
  let keyMaterial = $state<KeyMaterial | null>(null);
  let materialLoading = $state(false);
  let materialError = $state("");
  let inspectorMenuOpen = $state(false);
  let collectionView = $state<CollectionView>("grid");
  let collectionSort = $state<CollectionSort>("az");
  let collectionQuery = $state("");
  const keys = $derived.by(() =>
    identities
      .filter(
        (identity) =>
          identity.kind === "key_file" &&
          `${identity.label} ${identity.kind}`.toLowerCase().includes(collectionQuery.trim().toLowerCase())
      )
      .sort((left, right) =>
        compareCollectionItems(
          left.label,
          right.label,
          left.created_at,
          right.created_at,
          collectionSort
        )
      )
  );
  const reusableIdentities = $derived.by(() =>
    identities
      .filter(
        (identity) =>
          identity.kind !== "key_file" &&
          `${identity.label} ${identity.kind}`.toLowerCase().includes(collectionQuery.trim().toLowerCase())
      )
      .sort((left, right) =>
        compareCollectionItems(
          left.label,
          right.label,
          left.created_at,
          right.created_at,
          collectionSort
        )
      )
  );

  onMount(() => {
    void refresh();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    if ("__TAURI_INTERNALS__" in window) {
      void getCurrentWindow()
        .onDragDropEvent(({ payload }) => {
          if (payload.type === "enter" || payload.type === "over") {
            dragActive = true;
          } else if (payload.type === "leave") {
            dragActive = false;
          } else {
            dragActive = false;
            void importDroppedKeys(payload.paths);
          }
        })
        .then((stop) => {
          if (disposed) stop();
          else unlisten = stop;
        })
        .catch((cause) => {
          importStatus = `File drop is unavailable: ${errorMessage(cause)}`;
          importError = true;
        });
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function refresh(preferredId?: string) {
    loading = true;
    try {
      identities = await listIdentities();
      const next = preferredId
        ? identities.find((identity) => identity.id === preferredId) ?? null
        : selected
          ? identities.find((identity) => identity.id === selected?.id) ?? null
          : null;
      selected = next ? { ...next } : null;
      await loadMaterial(selected);
      pendingSecret = "";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      loading = false;
    }
  }

  function createIdentity(kind: IdentityKind = "password") {
    generationOpen = false;
    importOpen = false;
    newKeyMenuOpen = false;
    selected = {
      id: crypto.randomUUID(),
      label: kind === "password" ? "Password" : "",
      kind,
      username: null,
      key_path: null,
      secret_stored: false,
      created_at: new Date().toISOString()
    };
    keyMaterial = null;
    materialError = "";
    inspectorMenuOpen = false;
    pendingSecret = "";
    message =
      kind === "password"
          ? "The password will be encrypted by your operating-system keyring."
          : "Drop a private key into Keychain to copy it into the Heminus vault.";
    isError = false;
  }

  function openGenerator() {
    selected = null;
    keyMaterial = null;
    importOpen = false;
    newKeyMenuOpen = false;
    generationOpen = true;
    message = "";
    generatedLabel = "";
    generatedPassphrase = "";
  }

  function openImporter() {
    selected = null;
    keyMaterial = null;
    generationOpen = false;
    importOpen = true;
    newKeyMenuOpen = false;
    importPath = "";
    importLabel = "";
    importPassphrase = "";
    importStatus = "";
    importError = false;
  }

  async function chooseImportFile() {
    try {
      const selectedPath = await openDialog({
        multiple: false,
        directory: false,
        title: "Import a private key into the Heminus vault"
      });
      if (!selectedPath) return;
      importPath = selectedPath;
      if (!importLabel.trim()) {
        importLabel = selectedPath.split(/[\\/]/).pop() || "Imported key";
      }
    } catch (cause) {
      importStatus = errorMessage(cause);
      importError = true;
    }
  }

  async function importSelectedKey() {
    if (!importPath || importing) return;
    importing = true;
    importStatus = "Inspecting private key…";
    importError = false;
    try {
      const imported = await importPrivateKey(importPath);
      let identity = imported.identity;
      if (importLabel.trim() && importLabel.trim() !== identity.label) {
        identity = await saveIdentity({ ...identity, label: importLabel.trim() });
      }
      if (imported.encrypted) {
        if (!importPassphrase) {
          throw new Error("This private key is encrypted. Enter its passphrase to import it.");
        }
        await setIdentitySecret(identity.id, importPassphrase);
      }
      importPassphrase = "";
      importOpen = false;
      await refresh(identity.id);
      message = imported.alreadyPresent
        ? "This private key was already present in the Heminus vault"
        : `${imported.format} private key imported into the Heminus vault`;
      if (imported.encrypted && !identity.secret_stored) {
        message += " · enter its passphrase in the inspector";
      }
      isError = false;
    } catch (cause) {
      importStatus = errorMessage(cause);
      importError = true;
    } finally {
      importing = false;
    }
  }

  async function generateKey() {
    if (saving) return;
    saving = true;
    try {
      const identity = await generateSshKey(
        generatedLabel,
        "",
        "",
        generatedPassphrase || null
      );
      generatedPassphrase = "";
      generationOpen = false;
      await refresh(identity.id);
      message = "Ed25519 key pair generated in the Heminus vault";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      saving = false;
    }
  }

  async function importDroppedKeys(paths: string[]) {
    if (importing || paths.length === 0) return;
    importing = true;
    importStatus = "Inspecting private key…";
    importError = false;
    const imported = [];
    const failures: string[] = [];
    for (const path of paths) {
      try {
        imported.push(await importPrivateKey(path));
      } catch (cause) {
        const name = path.split(/[\\/]/).pop() || path;
        failures.push(`${name}: ${errorMessage(cause)}`);
      }
    }

    const last = imported.at(-1);
    if (last) {
      importOpen = false;
      generationOpen = false;
      await refresh(last.identity.id);
    }
    const added = imported.filter((result) => !result.alreadyPresent).length;
    const existing = imported.length - added;
    const descriptions = [...new Set(imported.map((result) => result.format))].join(", ");
    const summary = [
      added > 0 ? `${added} private ${added === 1 ? "key" : "keys"} imported` : "",
      existing > 0 ? `${existing} already saved` : "",
      descriptions ? `Format: ${descriptions}` : ""
    ].filter(Boolean);
    importStatus = failures.length > 0 ? failures.join(" · ") : summary.join(" · ");
    importError = failures.length > 0;
    if (last?.encrypted && !last.identity.secret_stored) {
      message = "Encrypted key copied into the vault. Open New key and import it again to save its passphrase.";
      isError = false;
    }
    importing = false;
  }

  async function persist() {
    if (!selected || saving) return;
    saving = true;
    try {
      const saved = await saveIdentity(selected);
      if (pendingSecret) {
        await setIdentitySecret(saved.id, pendingSecret);
      }
      pendingSecret = "";
      await refresh(saved.id);
      message =
        saved.kind === "key_file"
          ? "Key details saved"
          : saved.kind === "password"
            ? "Password identity saved"
            : "Identity saved";
      isError = false;
    } catch (cause) {
      showError(cause);
    } finally {
      saving = false;
    }
  }

  async function removeStoredSecret() {
    if (!selected || !selected.secret_stored) return;
    try {
      await deleteIdentitySecret(selected.id);
      await refresh(selected.id);
      message = "Stored credential removed from the system keyring";
      isError = false;
    } catch (cause) {
      showError(cause);
    }
  }

  async function remove() {
    if (!selected || !identities.some((identity) => identity.id === selected?.id)) return;
    if (
      !(await confirmDialog({
        title: "Remove identity?",
        message: `Remove “${selected.label}” from the Heminus vault?`,
        confirmLabel: "Remove",
        danger: true
      }))
    ) return;
    try {
      await deleteIdentity(selected.id);
      await refresh();
      message = "";
    } catch (cause) {
      showError(cause);
    }
  }

  function choose(identity: Identity) {
    generationOpen = false;
    importOpen = false;
    newKeyMenuOpen = false;
    selected = { ...identity };
    inspectorMenuOpen = false;
    pendingSecret = "";
    message = "";
    void loadMaterial(selected);
  }

  async function loadMaterial(identity: Identity | null) {
    keyMaterial = null;
    materialError = "";
    if (
      !identity ||
      identity.kind !== "key_file" ||
      !identities.some((candidate) => candidate.id === identity.id)
    ) return;
    materialLoading = true;
    try {
      keyMaterial = await readKeyMaterial(identity.id);
    } catch (cause) {
      materialError = errorMessage(cause);
    } finally {
      materialLoading = false;
    }
  }

  function closeInspector() {
    selected = null;
    generationOpen = false;
    importOpen = false;
    inspectorMenuOpen = false;
    keyMaterial = null;
    materialError = "";
    message = "";
  }

  function showError(cause: unknown) {
    message = errorMessage(cause);
    isError = true;
  }

  function errorMessage(cause: unknown) {
    return cause instanceof Error ? cause.message : String(cause);
  }
</script>

<section class="manager-page" class:with-inspector={Boolean(selected || importOpen || generationOpen)}>
  <div class="manager-main">
    <header class="content-toolbar">
      <div class="split-create">
        <button class="secondary-action split-create-main" onclick={openImporter}>
          <Icon name="plus" /> New key
        </button>
        <button
          class="secondary-action split-create-toggle"
          class:active={newKeyMenuOpen}
          title="More key options"
          onclick={() => (newKeyMenuOpen = !newKeyMenuOpen)}
        ><Icon name="chevron" size={16} /></button>
        {#if newKeyMenuOpen}
          <div class="create-dropdown">
            <button onclick={openGenerator}><Icon name="key" size={17} /><span>Generate key</span></button>
            <button onclick={() => createIdentity("password")}><Icon name="shield" size={17} /><span>New Identity</span></button>
          </div>
        {/if}
      </div>
      <div class="toolbar-spacer"></div>
      <CollectionControls
        view={collectionView}
        sort={collectionSort}
        onviewchange={(view) => (collectionView = view)}
        onsortchange={(sort) => (collectionSort = sort)}
        searchValue={collectionQuery}
        searchPlaceholder="Search keychain"
        onsearchchange={(value) => (collectionQuery = value)}
      />
      <span class="manager-count">{identities.length} saved in vault</span>
    </header>
    <div class="manager-scroll">
      <div class="page-heading">
        <div>
          <h1>Keychain</h1>
          <p>Keys are copied into the private Heminus vault; passwords and passphrases use its namespaced OS keyring.</p>
        </div>
      </div>
      {#if dragActive}
        <div class="key-drop-overlay">
          <Icon name="key" size={27} />
          <strong>Drop private key files to import</strong>
        </div>
      {/if}
      {#if loading}
        <div class="loading-row">Loading identities…</div>
      {:else if identities.length === 0}
        <div class="empty-page manager-empty">
          <div class="empty-glyph"><Icon name="key" size={30} /></div>
          <h2>Add an SSH identity</h2>
          <p>Generate an Ed25519 key, drop a private key to import it, or store a password.</p>
          <button class="secondary-action" onclick={openImporter}>New key</button>
        </div>
      {:else}
        {#if keys.length === 0 && reusableIdentities.length === 0}
          <div class="empty-page manager-empty">
            <div class="empty-glyph"><Icon name="search" size={28} /></div>
            <h2>No matching identities</h2>
            <p>Try another key or identity name.</p>
          </div>
        {/if}
        {#if keys.length > 0}
          <section class="manager-section">
            <h2>Keys</h2>
            <div class="manager-grid" class:list-view={collectionView === "list"}>
              {#each keys as identity (identity.id)}
                <button
                  class="manager-card"
                  class:selected={selected?.id === identity.id}
                  onclick={() => choose(identity)}
                >
                  <span class="manager-icon"><Icon name="key" /></span>
                  <span>
                    <strong>{identity.label}</strong>
                    <small>{identity.secret_stored ? "Private key · passphrase stored" : "Private key · Heminus vault"}</small>
                  </span>
                </button>
              {/each}
            </div>
          </section>
        {/if}
        {#if reusableIdentities.length > 0}
          <section class="manager-section">
            <h2>Identities</h2>
            <div class="manager-grid" class:list-view={collectionView === "list"}>
              {#each reusableIdentities as identity (identity.id)}
                <button
                  class="manager-card"
                  class:selected={selected?.id === identity.id}
                  onclick={() => choose(identity)}
                >
                  <span class="manager-icon"><Icon name="shield" /></span>
                  <span>
                    <strong>{identity.label}</strong>
                    <small>
                      {identity.kind === "agent"
                        ? "Legacy system-agent identity"
                        : identity.secret_stored ? "Password · native keyring" : "Password not stored"}
                    </small>
                  </span>
                </button>
              {/each}
            </div>
          </section>
        {/if}
      {/if}

    </div>
  </div>

  {#if selected || importOpen || generationOpen}
  <aside class="manager-inspector">
    <header>
      <div class="manager-inspector-heading">
        <strong>
          {importOpen
            ? "New Key"
            : generationOpen ? "Generate Ed25519 Key" : selected?.label || "New Identity"}
        </strong>
        <span>Personal vault</span>
      </div>
      <div class="manager-inspector-header-actions">
        {#if selected && identities.some((identity) => identity.id === selected?.id)}
          <div class="host-editor-menu-anchor">
            <button
              class="icon-button"
              class:active={inspectorMenuOpen}
              title="Identity actions"
              onclick={() => (inspectorMenuOpen = !inspectorMenuOpen)}
            ><Icon name="more" /></button>
            {#if inspectorMenuOpen}
              <div class="host-editor-menu">
                {#if selected.secret_stored}
                  <button onclick={() => void removeStoredSecret()}>
                    <Icon name="shield" /> Remove stored secret
                  </button>
                {/if}
                <button class="danger-copy" onclick={() => void remove()}>
                  <Icon name="trash" /> Remove
                </button>
              </div>
            {/if}
          </div>
        {/if}
        {#if selected || importOpen || generationOpen}
          <button class="icon-button" title="Close editor" onclick={closeInspector}>
            <Icon name="close" />
          </button>
        {/if}
      </div>
    </header>
    {#if importOpen}
      <div class="inspector-scroll">
        <section class="form-card key-import-form">
          <h2>Import private key</h2>
          <p class="security-note">
            The source stays untouched. Heminus validates and copies the key into its private vault.
          </p>
          <label for="import-key-label">Label</label>
          <input id="import-key-label" bind:value={importLabel} placeholder="Production key" />
          <label for="import-key-path">Private key</label>
          <textarea
            id="import-key-path"
            readonly
            value={importPath}
            placeholder="Choose or drop a private key file"
          ></textarea>
          <div
            class="inspector-key-drop"
            class:active={dragActive}
            class:busy={importing}
          >
            <Icon name="key" size={24} />
            <span>
              <strong>{dragActive ? "Drop the key now" : "Drag and drop a private key file"}</strong>
              <small>OpenSSH, RSA, DSA, EC, and PKCS#8 PEM</small>
            </span>
          </div>
          <button class="secondary-action import-file-action" onclick={() => void chooseImportFile()}>
            <Icon name="folder" size={16} /> Choose key file
          </button>
          <label for="import-key-passphrase">Passphrase</label>
          <input
            id="import-key-passphrase"
            type="password"
            autocomplete="new-password"
            bind:value={importPassphrase}
            placeholder="Only for an encrypted private key"
          />
          <p class="security-note">
            Leave this empty for an unencrypted key.
          </p>
          {#if importStatus}
            <p class:error-message={importError} class="form-message">{importStatus}</p>
          {/if}
        </section>
      </div>
      <footer class="manager-inspector-footer">
        <button
          class="manager-primary-action"
          disabled={importing || !importPath}
          onclick={() => void importSelectedKey()}
        >{importing ? "Importing…" : "Import Key"}</button>
      </footer>
    {:else if generationOpen}
      <div class="inspector-scroll">
        <section class="form-card">
          <h2>New key pair</h2>
          <p class="security-note">
            Heminus generates the Ed25519 key files and public-key metadata automatically.
          </p>
          <label for="generated-label">Name</label>
          <input
            id="generated-label"
            bind:value={generatedLabel}
            placeholder="My laptop key"
            autocomplete="off"
          />
          <label for="generated-passphrase">Passphrase</label>
          <input
            id="generated-passphrase"
            type="password"
            autocomplete="new-password"
            bind:value={generatedPassphrase}
            placeholder="Optional"
          />
          <p class="security-note">
            Optional password that encrypts this private key. It is not the SSH server password.
          </p>
        </section>
      </div>
      <footer class="manager-inspector-footer">
        <button
          class="manager-primary-action"
          disabled={saving || !generatedLabel.trim()}
          onclick={generateKey}
        >
          {saving ? "Generating…" : "Generate Key"}
        </button>
      </footer>
    {:else if selected}
      <div class="inspector-scroll">
        <section class="form-card">
          <h2>Authentication</h2>
          <p class="security-note">
            {selected.kind === "key_file"
              ? "Private key stored in the Heminus vault"
              : selected.kind === "password"
                ? "Password stored in the Heminus credential namespace"
                : "Legacy system-agent identity"}
          </p>
          <label for="identity-label">Name</label>
          <input id="identity-label" bind:value={selected.label} placeholder="Production credential" />
          {#if selected.kind === "key_file"}
            <label for="identity-private-key">Private key</label>
            <textarea
              id="identity-private-key"
              class="key-material-field private"
              value={keyMaterial?.privateKey ?? ""}
              readonly
              spellcheck="false"
              placeholder={materialLoading ? "Loading key material…" : "Private key is unavailable"}
            ></textarea>
            <label for="identity-public-key">Public key</label>
            <textarea
              id="identity-public-key"
              class="key-material-field"
              value={keyMaterial?.publicKey ?? ""}
              readonly
              spellcheck="false"
              placeholder={materialLoading ? "Loading public key…" : "No public key saved or derivable"}
            ></textarea>
            <label for="identity-certificate">Certificate</label>
            <textarea
              id="identity-certificate"
              class="key-material-field"
              value={keyMaterial?.certificate ?? ""}
              readonly
              spellcheck="false"
              placeholder="No SSH certificate attached"
            ></textarea>
            {#if materialError}<p class="error-message">{materialError}</p>{/if}
          {/if}
          {#if selected.kind === "agent"}
            <p class="error-message">
              This legacy identity depended on the system SSH agent. Import its private key into
              Heminus and assign the imported identity to your hosts.
            </p>
          {/if}
          {#if selected.kind === "password"}
            <label for="identity-secret">Password</label>
            <input
              id="identity-secret"
              type="password"
              autocomplete="new-password"
              bind:value={pendingSecret}
              placeholder={selected.secret_stored ? "Stored · enter to replace" : "Not stored"}
            />
            <p class="security-note">
              {selected.secret_stored
                ? "A password is saved for one-click access."
                : "Enter the SSH server password to save it for one-click access."}
            </p>
          {/if}
        </section>
        {#if message}<p class:error-message={isError} class="form-message">{message}</p>{/if}
      </div>
      <footer class="manager-inspector-footer">
        <button
          class="manager-primary-action"
          disabled={saving || selected.kind === "agent"}
          onclick={persist}
        >
          <Icon name="save" /> {saving ? "Saving…" : "Save"}
        </button>
      </footer>
    {:else}
      <div class="empty-inspector">
        <Icon name="key" size={32} />
        <p>Select an identity or generate a new key.</p>
      </div>
    {/if}
  </aside>
  {/if}
</section>
