<script lang="ts">
  import Icon from "./Icon.svelte";

  export interface SelectOption {
    value: string;
    label: string;
    disabled?: boolean;
  }

  let {
    value = "",
    options,
    onchange,
    ariaLabel = "Select an option",
    disabled = false
  }: {
    value?: string;
    options: SelectOption[];
    onchange: (value: string) => void;
    ariaLabel?: string;
    disabled?: boolean;
  } = $props();

  let open = $state(false);
  let trigger = $state<HTMLButtonElement>();
  let menuStyle = $state("");
  const selectedLabel = $derived(
    options.find((option) => option.value === value)?.label ?? options[0]?.label ?? ""
  );

  function toggle(event: MouseEvent) {
    event.stopPropagation();
    if (disabled) return;
    open = !open;
    if (open) positionMenu();
  }

  function positionMenu() {
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const estimatedHeight = Math.min(options.length * 36 + 12, 250);
    const openAbove =
      window.innerHeight - rect.bottom < estimatedHeight && rect.top > estimatedHeight;
    const top = openAbove
      ? Math.max(8, rect.top - estimatedHeight - 5)
      : Math.min(window.innerHeight - estimatedHeight - 8, rect.bottom + 5);
    menuStyle = `left:${rect.left}px;top:${top}px;width:${rect.width}px;`;
  }

  function choose(option: SelectOption) {
    if (option.disabled) return;
    onchange(option.value);
    open = false;
    trigger?.focus();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      open = false;
      trigger?.focus();
    }
  }
</script>

<svelte:window
  onclick={() => (open = false)}
  onresize={() => open && positionMenu()}
  onkeydown={handleKeydown}
/>

<span class="custom-select">
  <button
    bind:this={trigger}
    type="button"
    class="custom-select-trigger"
    class:open
    {disabled}
    aria-label={ariaLabel}
    aria-expanded={open}
    onclick={toggle}
  >
    <span>{selectedLabel}</span>
    <Icon name="chevron" size={14} />
  </button>
  {#if open}
    <span class="custom-select-menu" style={menuStyle} role="listbox" aria-label={ariaLabel}>
      {#each options as option (option.value)}
        <button
          type="button"
          role="option"
          aria-selected={option.value === value}
          class:selected={option.value === value}
          disabled={option.disabled}
          onclick={(event) => {
            event.stopPropagation();
            choose(option);
          }}
        >
          <span>{option.label}</span>
          {#if option.value === value}<span class="select-check">✓</span>{/if}
        </button>
      {/each}
    </span>
  {/if}
</span>
