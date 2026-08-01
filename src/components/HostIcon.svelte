<script lang="ts">
  import alpineLogo from "../../os-image/alpine.png";
  import archLogo from "../../os-image/arch.png";
  import debianLogo from "../../os-image/debian.png";
  import fedoraLogo from "../../os-image/fedora.png";
  import linuxLogo from "../../os-image/linux.png";
  import redhatLogo from "../../os-image/redhat.png";
  import suseLogo from "../../os-image/suse.png";
  import ubuntuLogo from "../../os-image/ubuntu.png";
  import Icon from "./Icon.svelte";
  import {
    hostOperatingSystems,
    type HostOperatingSystem
  } from "../lib/hostOperatingSystem";

  let { hostId, size = 18 }: { hostId?: string | null; size?: number } = $props();
  const brandIcons: Record<HostOperatingSystem, { src: string; title: string }> = {
    ubuntu: { src: ubuntuLogo, title: "Ubuntu" },
    debian: { src: debianLogo, title: "Debian" },
    fedora: { src: fedoraLogo, title: "Fedora" },
    redhat: { src: redhatLogo, title: "Red Hat" },
    arch: { src: archLogo, title: "Arch Linux" },
    alpine: { src: alpineLogo, title: "Alpine Linux" },
    suse: { src: suseLogo, title: "SUSE" },
    linux: { src: linuxLogo, title: "Linux" }
  };
  const operatingSystem = $derived(hostId ? $hostOperatingSystems[hostId] : null);
  const brandIcon = $derived(operatingSystem ? brandIcons[operatingSystem] : null);
</script>

{#if brandIcon}
  <img
    class="host-brand-icon"
    src={brandIcon.src}
    alt={brandIcon.title}
    width={size}
    height={size}
  />
{:else}
  <Icon name="terminal" {size} />
{/if}
