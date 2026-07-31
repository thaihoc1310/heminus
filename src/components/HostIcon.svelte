<script lang="ts">
  import {
    siAlpinelinux,
    siArchlinux,
    siDebian,
    siFedora,
    siLinux,
    siOpensuse,
    siRedhat,
    siUbuntu,
    type SimpleIcon
  } from "simple-icons";
  import Icon from "./Icon.svelte";
  import {
    hostOperatingSystems,
    type HostOperatingSystem
  } from "../lib/hostOperatingSystem";

  let { hostId, size = 18 }: { hostId?: string | null; size?: number } = $props();
  const brandIcons: Record<HostOperatingSystem, SimpleIcon> = {
    ubuntu: siUbuntu,
    debian: siDebian,
    fedora: siFedora,
    redhat: siRedhat,
    arch: siArchlinux,
    alpine: siAlpinelinux,
    suse: siOpensuse,
    linux: siLinux
  };
  const operatingSystem = $derived(hostId ? $hostOperatingSystems[hostId] : null);
  const brandIcon = $derived(operatingSystem ? brandIcons[operatingSystem] : null);
</script>

{#if brandIcon}
  <svg
    class="host-brand-icon"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill={`#${brandIcon.hex}`}
    role="img"
    aria-label={brandIcon.title}
  >
    <path d={brandIcon.path}></path>
  </svg>
{:else}
  <Icon name="terminal" {size} />
{/if}
