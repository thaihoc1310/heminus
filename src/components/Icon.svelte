<script lang="ts">
  let {
    name,
    size = 18,
    strokeWidth = 1.8
  }: { name: string; size?: number; strokeWidth?: number } = $props();

  const paths: Record<string, string> = {
    menu: "M4 6h16M4 12h16M4 18h16",
    vault: "M4 7.5A2.5 2.5 0 0 1 6.5 5h11A2.5 2.5 0 0 1 20 7.5v9a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5zM8 9h8M8 13h5",
    folder: "M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    terminal: "M4 5h16v14H4zM7 9l3 3-3 3M12 15h4",
    plus: "M12 5v14M5 12h14",
    bell: "M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4",
    sidebar: "M4 4h16v16H4zM15 4v16M17.5 8h.01M17.5 12h.01",
    grid: "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z",
    list: "M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01",
    calendar: "M5 4h14a2 2 0 0 1 2 2v14H3V6a2 2 0 0 1 2-2zM8 2v4M16 2v4M3 9h18",
    sort: "M4 6h10M4 12h7M4 18h4M18 4v16M15 17l3 3 3-3",
    "sort-az": "M5 4h6M5 8h4M5 12h2M15 5h4l-4 7h4M15 16h4M17 14v6",
    "sort-za": "M5 4h6M5 8h4M5 12h2M15 4h4M17 2v6M15 13h4l-4 7h4",
    check: "M5 12l4 4L19 6",
    server: "M4 5h16v5H4zM4 14h16v5H4zM7 7.5h.01M7 16.5h.01M16 7.5h1M16 16.5h1",
    key: "M15.5 7.5a4 4 0 1 0-3.8 5.3L19 20l2-2-2-2 1.5-1.5-2-2-1.5 1.5-2.3-2.3a4 4 0 0 0 .8-3.2z",
    forward: "M4 7h12M13 4l3 3-3 3M20 17H8M11 14l-3 3 3 3",
    code: "M8 9l-3 3 3 3M16 9l3 3-3 3M14 5l-4 14",
    file: "M6 3h8l4 4v14H6zM14 3v5h5",
    "file-text": "M6 3h8l4 4v14H6zM14 3v5h5M9 12h6M9 16h6",
    "file-pdf": "M6 3h8l4 4v14H6zM14 3v5h5M8.5 16v-4h1.2a1.3 1.3 0 0 1 0 2.6H8.5M12 16v-4h1a2 2 0 0 1 0 4zM16 16v-4h2",
    "file-image": "M6 3h8l4 4v14H6zM14 3v5h5M8 17l2.5-3 2 2 1.5-2 2 3M10 10h.01",
    "file-sheet": "M6 3h8l4 4v14H6zM14 3v5h5M8 11h8M8 15h8M11 10v8",
    "file-presentation": "M6 3h8l4 4v14H6zM14 3v5h5M9 17v-6h6v6zM12 17v3",
    "file-video": "M6 3h8l4 4v14H6zM14 3v5h5M10 11l5 3-5 3z",
    "file-audio": "M6 3h8l4 4v14H6zM14 3v5h5M14 11v6M14 12l-4 1v5M10 18a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0M17 17a1.5 1.5 0 1 1-3 0",
    archive: "M5 4h14v5H5zM6 9h12v11H6zM10 13h4",
    shield: "M12 3l8 3v5c0 5-3.4 8.5-8 10-4.6-1.5-8-5-8-10V6zM9 12l2 2 4-4",
    fingerprint: "M12 10a2 2 0 0 0-2 2c0 1.02-.1 2.51-.26 4M14 13.12c0 2.38 0 6.38-1 8.88M17.29 21.02c.12-.6.43-2.3.5-3.02M2 12a10 10 0 0 1 18-6M2 16h.01M21.8 16c.2-2 .13-5.35 0-6M5 19.5c.5-1.5 1-5 1-7.5a7 7 0 0 1 .34-2M8.65 22c.21-.66.45-1.32.57-2M9 6.8a6 6 0 0 1 9 5.2v2",
    clock: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2",
    search: "M11 18a7 7 0 1 1 0-14 7 7 0 0 1 0 14zM20 20l-4-4",
    tag: "M3 11V5a2 2 0 0 1 2-2h6l10 10-8 8zM7.5 7.5h.01",
    more: "M5 12h.01M12 12h.01M19 12h.01",
    trash: "M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6",
    save: "M5 3h12l2 2v16H5zM8 3v6h8V3M8 21v-8h8v8",
    connect: "M8 12h12M16 8l4 4-4 4M4 5v14",
    "arrow-left": "M19 12H5M11 6l-6 6 6 6",
    "arrow-down": "M12 4v16M6 14l6 6 6-6",
    chevron: "M9 18l6-6-6-6",
    "chevron-right": "M9 18l6-6-6-6",
    edit: "M4 20h4l11-11-4-4L4 16zM13.5 6.5l4 4",
    copy: "M8 8h11v11H8zM5 16H4V4h12v1",
    paste: "M9 5h6M9 3h6v4H9zM7 5H5v16h14V5h-2",
    sun: "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.65 17.65l1.42 1.42M2 12h2M20 12h2M4.93 19.07l1.42-1.42M17.65 6.35l1.42-1.42",
    appearance: "M4 6h9M17 6h3M4 12h3M11 12h9M4 18h7M15 18h5M13 4v4M7 10v4M11 16v4",
    detach: "M14 4h6v6M20 4l-9 9M10 7H5v12h12v-5",
    eye: "M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12zM12 9a3 3 0 1 1 0 6 3 3 0 0 1 0-6z",
    "eye-off": "M3 3l18 18M10.6 6.2A10 10 0 0 1 12 6c6.5 0 10 6 10 6a16 16 0 0 1-2.2 2.8M14.1 14.1A3 3 0 0 1 9.9 9.9M6.2 6.2A15 15 0 0 0 2 12s3.5 6 10 6a10 10 0 0 0 5.8-1.8",
    minimize: "M6 12h12",
    maximize: "M6 6h12v12H6z",
    close: "M7 7l10 10M17 7 7 17",
    stop: "M7 7h10v10H7z",
    alert: "M12 3.6 2.7 19.4h18.6zM12 10v4M12 17h.01"
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name] ?? paths.terminal}></path>
</svg>
