import { writable } from "svelte/store";

export type HostOperatingSystem =
  | "ubuntu"
  | "debian"
  | "fedora"
  | "redhat"
  | "arch"
  | "alpine"
  | "suse"
  | "linux";

const storageKey = "heminus-host-operating-systems";

function load(): Record<string, HostOperatingSystem> {
  if (typeof localStorage === "undefined") return {};
  try {
    return JSON.parse(localStorage.getItem(storageKey) ?? "{}") as Record<string, HostOperatingSystem>;
  } catch {
    return {};
  }
}

export const hostOperatingSystems = writable<Record<string, HostOperatingSystem>>(load());

export function rememberHostOperatingSystem(hostId: string, operatingSystem: HostOperatingSystem) {
  hostOperatingSystems.update((current) => {
    if (current[hostId] === operatingSystem) return current;
    const next = { ...current, [hostId]: operatingSystem };
    localStorage.setItem(storageKey, JSON.stringify(next));
    return next;
  });
}

export function detectHostOperatingSystem(output: string): HostOperatingSystem | null {
  const value = output.toLowerCase();
  if (/ubuntu|linux mint|pop!_os/.test(value)) return "ubuntu";
  if (/debian|kali linux/.test(value)) return "debian";
  if (/fedora/.test(value)) return "fedora";
  if (/red hat|rhel|centos|rocky linux|almalinux|oracle linux|amazon linux/.test(value)) return "redhat";
  if (/arch linux|manjaro/.test(value)) return "arch";
  if (/alpine linux/.test(value)) return "alpine";
  if (/opensuse|suse linux/.test(value)) return "suse";
  return null;
}

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    if (event.key === storageKey) hostOperatingSystems.set(load());
  });
}
