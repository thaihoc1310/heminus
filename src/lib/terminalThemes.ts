import type { ITheme } from "@xterm/xterm";
import type { TerminalTheme } from "./types";

export interface TerminalThemeDefinition {
  id: TerminalTheme;
  label: string;
  description: string;
  preview: [string, string, string, string];
  palette: ITheme;
  chrome: {
    headerBackground: string;
    headerForeground: string;
    headerMuted: string;
    headerBorder: string;
    controlHover: string;
    activeForeground: string;
    activeBackground: string;
  };
}

function alpha(color: string, opacity: string): string {
  return /^#[0-9a-f]{6}$/i.test(color) ? `${color}${opacity}` : color;
}

/** WCAG contrast ratio between two #rrggbb colors. */
export function contrastRatio(a: string, b: string): number {
  const luminance = (hex: string) => {
    const [r, g, bl] = [1, 3, 5].map((index) => {
      const channel = parseInt(hex.slice(index, index + 2), 16) / 255;
      return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

/** `weight` of `color` over `base`, both #rrggbb. */
function mix(color: string, base: string, weight: number): string {
  if (!/^#[0-9a-f]{6}$/i.test(color) || !/^#[0-9a-f]{6}$/i.test(base)) return color;
  const channel = (hex: string, index: number) => parseInt(hex.slice(1 + index * 2, 3 + index * 2), 16);
  return `#${[0, 1, 2]
    .map((index) =>
      Math.round(channel(color, index) * weight + channel(base, index) * (1 - weight))
        .toString(16)
        .padStart(2, "0")
    )
    .join("")}`;
}

function defineTheme(
  id: TerminalTheme,
  label: string,
  description: string,
  palette: ITheme,
  /** The theme's signature color, when brightBlue is not it. */
  signature?: string
): TerminalThemeDefinition {
  const background = palette.background ?? "#1e2228";
  const foreground = palette.foreground ?? "#c9d1d9";
  const muted = palette.brightBlack ?? foreground;
  // brightBlack is often close to the background (Solarized Dark uses the
  // background itself), too faint for labels and secondary text.
  // Start well below the foreground and move toward it until labels read.
  let chromeMuted = mix(foreground, background, 0.6);
  for (let step = 13; step <= 20 && contrastRatio(chromeMuted, background) < 3; step += 1) {
    chromeMuted = mix(foreground, background, step / 20);
  }
  const accent = signature ?? palette.brightBlue ?? palette.blue ?? foreground;
  const completePalette: ITheme = {
    ...palette,
    cursor: palette.cursor ?? accent,
    cursorAccent: palette.cursorAccent ?? background,
    selectionBackground: palette.selectionBackground ?? alpha(accent, "55"),
    selectionInactiveBackground:
      palette.selectionInactiveBackground ?? alpha(muted, "45"),
    scrollbarSliderBackground: palette.scrollbarSliderBackground ?? alpha(foreground, "24"),
    scrollbarSliderHoverBackground:
      palette.scrollbarSliderHoverBackground ?? alpha(foreground, "42"),
    scrollbarSliderActiveBackground:
      palette.scrollbarSliderActiveBackground ?? alpha(accent, "70"),
    overviewRulerBorder: palette.overviewRulerBorder ?? alpha(foreground, "20")
  };
  return {
    id,
    label,
    description,
    preview: [
      background,
      foreground,
      palette.brightGreen ?? palette.green ?? foreground,
      accent
    ],
    palette: completePalette,
    chrome: {
      // Pane chrome intentionally shares the exact terminal background.
      headerBackground: background,
      headerForeground: foreground,
      headerMuted: chromeMuted,
      headerBorder: alpha(foreground, "20"),
      controlHover: alpha(foreground, "12"),
      activeForeground: accent,
      activeBackground: alpha(accent, "30")
    }
  };
}

export const terminalThemes: TerminalThemeDefinition[] = [
  defineTheme("heminus_dark", "Heminus Dark", "Neutral graphite", {
    background: "#1e2228", foreground: "#c9d1d9", cursor: "#e6edf3", cursorAccent: "#1e2228",
    selectionBackground: "#264f7866", selectionInactiveBackground: "#44505d55",
    black: "#24292f", red: "#ff7b72", green: "#7ee787", yellow: "#d2a85a",
    blue: "#58a6ff", magenta: "#bc8cff", cyan: "#76e3ea", white: "#b1bac4",
    brightBlack: "#6e7681", brightRed: "#ffa198", brightGreen: "#aff5b4",
    brightYellow: "#eac777", brightBlue: "#79c0ff", brightMagenta: "#d2a8ff",
    brightCyan: "#a5f3fc", brightWhite: "#f0f6fc"
  }),
  defineTheme("paper_light", "Heminus Light", "Clean daylight", {
    background: "#f7f3e8", foreground: "#282b32", cursor: "#1e5d87", cursorAccent: "#f7f3e8",
    selectionBackground: "#9ec7e877", selectionInactiveBackground: "#b8c9d466",
    black: "#282b32", red: "#b74343", green: "#648f55", yellow: "#9b752d",
    blue: "#356fa3", magenta: "#7b5aa6", cyan: "#397f81", white: "#ded9cb",
    brightBlack: "#777b80", brightRed: "#d45b5b", brightGreen: "#78a866",
    brightYellow: "#b88b38", brightBlue: "#4c86b8", brightMagenta: "#9370bd",
    brightCyan: "#4c989a", brightWhite: "#ffffff"
  }),
  defineTheme("flexoki_dark", "Flexoki Dark", "Inky and warm", {
    background: "#100f0f", foreground: "#cecdc3", cursor: "#cecdc3", cursorAccent: "#100f0f",
    selectionBackground: "#403e3caa",
    black: "#100f0f", red: "#af3029", green: "#66800b", yellow: "#ad8301",
    blue: "#205ea6", magenta: "#a02f6f", cyan: "#24837b", white: "#cecdc3",
    brightBlack: "#575653", brightRed: "#d14d41", brightGreen: "#879a39",
    brightYellow: "#d0a215", brightBlue: "#4385be", brightMagenta: "#ce5d97",
    brightCyan: "#3aa99f", brightWhite: "#fffcf0"
  }),
  defineTheme("flexoki_light", "Flexoki Light", "Warm paper and ink", {
    background: "#fffcf0", foreground: "#100f0f", cursor: "#100f0f", cursorAccent: "#fffcf0",
    selectionBackground: "#cecdc388",
    black: "#100f0f", red: "#af3029", green: "#66800b", yellow: "#ad8301",
    blue: "#205ea6", magenta: "#a02f6f", cyan: "#24837b", white: "#cecdc3",
    brightBlack: "#6f6e69", brightRed: "#d14d41", brightGreen: "#879a39",
    brightYellow: "#d0a215", brightBlue: "#4385be", brightMagenta: "#ce5d97",
    brightCyan: "#3aa99f", brightWhite: "#fffcf0"
  }),
  defineTheme("kanagawa_wave", "Kanagawa Wave", "Deep indigo", {
    background: "#1f1f28", foreground: "#dcd7ba", cursor: "#c8c093", cursorAccent: "#1f1f28",
    selectionBackground: "#2d4f6788",
    black: "#16161d", red: "#c34043", green: "#76946a", yellow: "#c0a36e",
    blue: "#7e9cd8", magenta: "#957fb8", cyan: "#6a9589", white: "#c8c093",
    brightBlack: "#727169", brightRed: "#e82424", brightGreen: "#98bb6c",
    brightYellow: "#e6c384", brightBlue: "#7fb4ca", brightMagenta: "#938aa9",
    brightCyan: "#7aa89f", brightWhite: "#dcd7ba"
  }),
  defineTheme("kanagawa_lotus", "Kanagawa Lotus", "Soft parchment", {
    background: "#f2ecbc", foreground: "#545464", cursor: "#43436c", cursorAccent: "#f2ecbc",
    selectionBackground: "#c9cbd1aa",
    black: "#1f1f28", red: "#c84053", green: "#6f894e", yellow: "#77713f",
    blue: "#4d699b", magenta: "#b35b79", cyan: "#597b75", white: "#d5cea3",
    brightBlack: "#8a8980", brightRed: "#d7474b", brightGreen: "#6e915f",
    brightYellow: "#836f4a", brightBlue: "#6693bf", brightMagenta: "#a45c8a",
    brightCyan: "#5e857a", brightWhite: "#f6f0c9"
  }),
  defineTheme("gruvbox_dark", "Gruvbox Dark", "Warm and low contrast", {
    background: "#282828", foreground: "#ebdbb2", cursor: "#fe8019", cursorAccent: "#282828",
    selectionBackground: "#504945aa",
    black: "#282828", red: "#cc241d", green: "#98971a", yellow: "#d79921",
    blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#a89984",
    brightBlack: "#928374", brightRed: "#fb4934", brightGreen: "#b8bb26",
    brightYellow: "#fabd2f", brightBlue: "#83a598", brightMagenta: "#d3869b",
    brightCyan: "#8ec07c", brightWhite: "#ebdbb2"
  }),
  defineTheme("hacker_blue", "Hacker Blue", "Electric cyan", {
    background: "#020b1d", foreground: "#c8f1ff", cursor: "#26d9ff", cursorAccent: "#020b1d",
    selectionBackground: "#2f8cff66",
    black: "#061127", red: "#ff5c7c", green: "#53e6a6", yellow: "#ffd166",
    blue: "#2f8cff", magenta: "#b88cff", cyan: "#26d9ff", white: "#c8f1ff",
    brightBlack: "#50627f", brightRed: "#ff7994", brightGreen: "#76f2bd",
    brightYellow: "#ffe08a", brightBlue: "#62a8ff", brightMagenta: "#d0b0ff",
    brightCyan: "#71e9ff", brightWhite: "#ffffff"
  }, "#26d9ff"),
  defineTheme("hacker_green", "Hacker Green", "Phosphor green", {
    background: "#001208", foreground: "#8df59c", cursor: "#39ff5a", cursorAccent: "#001208",
    selectionBackground: "#20c84355",
    black: "#001208", red: "#ff5364", green: "#20c843", yellow: "#c8e34b",
    blue: "#47a8ff", magenta: "#c77dff", cyan: "#2ee6a6", white: "#a8dbad",
    brightBlack: "#3b6b47", brightRed: "#ff7582", brightGreen: "#53ff6e",
    brightYellow: "#e5f47b", brightBlue: "#78c1ff", brightMagenta: "#d8a5ff",
    brightCyan: "#69f5c6", brightWhite: "#eaffed"
  }, "#39ff5a"),
  defineTheme("hacker_red", "Hacker Red", "Scarlet phosphor", {
    background: "#190204", foreground: "#ffb3b3", cursor: "#ff303f", cursorAccent: "#190204",
    selectionBackground: "#e0182d55",
    black: "#190204", red: "#d9162f", green: "#57c785", yellow: "#e0bb4f",
    blue: "#628bd8", magenta: "#d05ca9", cyan: "#53c4c7", white: "#d9a7a7",
    brightBlack: "#744148", brightRed: "#ff3448", brightGreen: "#78dda0",
    brightYellow: "#f3d778", brightBlue: "#87a9ed", brightMagenta: "#e782c2",
    brightCyan: "#79dce0", brightWhite: "#fff0f0"
  }, "#ff5364"),
  defineTheme("rose_pine_moon", "Rosé Pine Moon", "Soft moonlit violet", {
    background: "#232136", foreground: "#e0def4", cursor: "#e0def4", cursorAccent: "#232136",
    selectionBackground: "#44415a77",
    black: "#393552", red: "#eb6f92", green: "#9ccfd8", yellow: "#f6c177",
    blue: "#3e8fb0", magenta: "#c4a7e7", cyan: "#ea9a97", white: "#e0def4",
    brightBlack: "#6e6a86", brightRed: "#eb6f92", brightGreen: "#9ccfd8",
    brightYellow: "#f6c177", brightBlue: "#3e8fb0", brightMagenta: "#c4a7e7",
    brightCyan: "#ea9a97", brightWhite: "#f4f2ff"
  }),
  defineTheme("rose_pine_dawn", "Rosé Pine Dawn", "Warm rose daylight", {
    background: "#faf4ed", foreground: "#575279", cursor: "#575279", cursorAccent: "#faf4ed",
    selectionBackground: "#dfdad9aa",
    black: "#575279", red: "#b4637a", green: "#56949f", yellow: "#ea9d34",
    blue: "#286983", magenta: "#907aa9", cyan: "#d7827e", white: "#f2e9e1",
    brightBlack: "#9893a5", brightRed: "#b4637a", brightGreen: "#56949f",
    brightYellow: "#ea9d34", brightBlue: "#286983", brightMagenta: "#907aa9",
    brightCyan: "#d7827e", brightWhite: "#fffaf3"
  }),
  defineTheme("catppuccin_mocha", "Catppuccin Mocha", "Pastel midnight", {
    background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", cursorAccent: "#1e1e2e",
    selectionBackground: "#585b7066",
    black: "#45475a", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af",
    blue: "#89b4fa", magenta: "#f5c2e7", cyan: "#94e2d5", white: "#bac2de",
    brightBlack: "#585b70", brightRed: "#f38ba8", brightGreen: "#a6e3a1",
    brightYellow: "#f9e2af", brightBlue: "#89b4fa", brightMagenta: "#f5c2e7",
    brightCyan: "#94e2d5", brightWhite: "#a6adc8"
  }),
  defineTheme("tokyo_night", "Tokyo Night", "Neon city night", {
    background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5", cursorAccent: "#1a1b26",
    selectionBackground: "#33467c77",
    black: "#15161e", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68",
    blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#a9b1d6",
    brightBlack: "#414868", brightRed: "#f7768e", brightGreen: "#9ece6a",
    brightYellow: "#e0af68", brightBlue: "#7aa2f7", brightMagenta: "#bb9af7",
    brightCyan: "#7dcfff", brightWhite: "#c0caf5"
  }),
  defineTheme("tokyo_day", "Tokyo Day", "Cool urban daylight", {
    background: "#e1e2e7", foreground: "#3760bf", cursor: "#3760bf", cursorAccent: "#e1e2e7",
    selectionBackground: "#b6bfe2aa",
    black: "#0f0f14", red: "#f52a65", green: "#587539", yellow: "#8c6c3e",
    blue: "#2e7de9", magenta: "#9854f1", cyan: "#007197", white: "#6172b0",
    brightBlack: "#9699a3", brightRed: "#f52a65", brightGreen: "#587539",
    brightYellow: "#8c6c3e", brightBlue: "#2e7de9", brightMagenta: "#9854f1",
    brightCyan: "#007197", brightWhite: "#3760bf"
  }),
  defineTheme("solarized_dark", "Solarized Dark", "Precision dark contrast", {
    background: "#002b36", foreground: "#839496", cursor: "#93a1a1", cursorAccent: "#002b36",
    selectionBackground: "#073642aa",
    black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
    blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
    brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75",
    brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4",
    brightCyan: "#93a1a1", brightWhite: "#fdf6e3"
  }, "#268bd2"),
  defineTheme("solarized_light", "Solarized Light", "Precision light contrast", {
    background: "#fdf6e3", foreground: "#657b83", cursor: "#586e75", cursorAccent: "#fdf6e3",
    selectionBackground: "#eee8d5cc",
    black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
    blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
    brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75",
    brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4",
    brightCyan: "#93a1a1", brightWhite: "#fdf6e3"
  }, "#268bd2"),
  defineTheme("dracula", "Dracula", "Classic violet dark", {
    background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f0", cursorAccent: "#282a36",
    selectionBackground: "#44475aaa",
    black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c",
    blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2",
    brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94",
    brightYellow: "#ffffa5", brightBlue: "#d6acff", brightMagenta: "#ff92df",
    brightCyan: "#a4ffff", brightWhite: "#ffffff"
  }),
  defineTheme("monokai", "Monokai", "Vivid studio dark", {
    background: "#272822", foreground: "#f8f8f2", cursor: "#f8f8f0", cursorAccent: "#272822",
    selectionBackground: "#49483eaa",
    black: "#272822", red: "#f92672", green: "#a6e22e", yellow: "#f4bf75",
    blue: "#66d9ef", magenta: "#ae81ff", cyan: "#a1efe4", white: "#f8f8f2",
    brightBlack: "#75715e", brightRed: "#f92672", brightGreen: "#a6e22e",
    brightYellow: "#e6db74", brightBlue: "#66d9ef", brightMagenta: "#ae81ff",
    brightCyan: "#a1efe4", brightWhite: "#f9f8f5"
  }),
  defineTheme("catppuccin_latte", "Catppuccin Latte", "Pastel morning", {
    background: "#eff1f5", foreground: "#4c4f69", cursor: "#dc8a78", cursorAccent: "#eff1f5",
    black: "#bcc0cc", red: "#d20f39", green: "#40a02b", yellow: "#df8e1d",
    blue: "#1e66f5", magenta: "#ea76cb", cyan: "#179299", white: "#5c5f77",
    brightBlack: "#acb0be", brightRed: "#e7103f", brightGreen: "#46b02f",
    brightYellow: "#e49931", brightBlue: "#3878f6", brightMagenta: "#ef95d7",
    brightCyan: "#19a1a8", brightWhite: "#6c6f85"
  }),
  defineTheme("gruvbox_light", "Gruvbox Light", "Warm retro paper", {
    background: "#fbf1c7", foreground: "#3c3836", cursor: "#3c3836", cursorAccent: "#fbf1c7",
    black: "#fbf1c7", red: "#cc241d", green: "#98971a", yellow: "#d79921",
    blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#7c6f64",
    brightBlack: "#928374", brightRed: "#9d0006", brightGreen: "#79740e",
    brightYellow: "#b57614", brightBlue: "#076678", brightMagenta: "#8f3f71",
    brightCyan: "#427b58", brightWhite: "#3c3836"
  }),
  defineTheme("nord", "Nord", "Arctic frost blue", {
    background: "#2e3440", foreground: "#d8dee9", cursor: "#eceff4", cursorAccent: "#2e3440",
    black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
    blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
    brightBlack: "#596377", brightRed: "#bf616a", brightGreen: "#a3be8c",
    brightYellow: "#ebcb8b", brightBlue: "#81a1c1", brightMagenta: "#b48ead",
    brightCyan: "#8fbcbb", brightWhite: "#eceff4"
  }),
  defineTheme("one_dark", "One Dark", "Balanced editor dark", {
    background: "#21252b", foreground: "#abb2bf", cursor: "#abb2bf", cursorAccent: "#21252b",
    black: "#21252b", red: "#e06c75", green: "#98c379", yellow: "#e5c07b",
    blue: "#61afef", magenta: "#c678dd", cyan: "#56b6c2", white: "#abb2bf",
    brightBlack: "#767676", brightRed: "#e06c75", brightGreen: "#98c379",
    brightYellow: "#e5c07b", brightBlue: "#61afef", brightMagenta: "#c678dd",
    brightCyan: "#56b6c2", brightWhite: "#abb2bf"
  }),
  defineTheme("everforest_dark", "Everforest Dark", "Calm forest green", {
    background: "#1e2326", foreground: "#d3c6aa", cursor: "#e69875", cursorAccent: "#1e2326",
    black: "#7a8478", red: "#e67e80", green: "#a7c080", yellow: "#dbbc7f",
    blue: "#7fbbb3", magenta: "#d699b6", cyan: "#83c092", white: "#f2efdf",
    brightBlack: "#a6b0a0", brightRed: "#f85552", brightGreen: "#8da101",
    brightYellow: "#dfa000", brightBlue: "#3a94c5", brightMagenta: "#df69ba",
    brightCyan: "#35a77c", brightWhite: "#fffbef"
  }, "#a7c080"),
  defineTheme("kanagawa_dragon", "Kanagawa Dragon", "Ink and ash", {
    background: "#181616", foreground: "#c5c9c5", cursor: "#c8c093", cursorAccent: "#181616",
    black: "#0d0c0c", red: "#c4746e", green: "#8a9a7b", yellow: "#c4b28a",
    blue: "#8ba4b0", magenta: "#a292a3", cyan: "#8ea4a2", white: "#c8c093",
    brightBlack: "#a6a69c", brightRed: "#e46876", brightGreen: "#87a987",
    brightYellow: "#e6c384", brightBlue: "#7fb4ca", brightMagenta: "#938aa9",
    brightCyan: "#7aa89f", brightWhite: "#c5c9c5"
  }),
  defineTheme("rose_pine", "Rosé Pine", "Muted pine night", {
    background: "#191724", foreground: "#e0def4", cursor: "#e0def4", cursorAccent: "#191724",
    black: "#26233a", red: "#eb6f92", green: "#31748f", yellow: "#f6c177",
    blue: "#9ccfd8", magenta: "#c4a7e7", cyan: "#ebbcba", white: "#e0def4",
    brightBlack: "#6e6a86", brightRed: "#eb6f92", brightGreen: "#31748f",
    brightYellow: "#f6c177", brightBlue: "#9ccfd8", brightMagenta: "#c4a7e7",
    brightCyan: "#ebbcba", brightWhite: "#e0def4"
  }),
  defineTheme("vesper", "Vesper", "Minimal amber dark", {
    background: "#101010", foreground: "#ffffff", cursor: "#acb1ab", cursorAccent: "#101010",
    black: "#101010", red: "#f5a191", green: "#90b99f", yellow: "#e6b99d",
    blue: "#aca1cf", magenta: "#e29eca", cyan: "#ea83a5", white: "#a0a0a0",
    brightBlack: "#7e7e7e", brightRed: "#ff8080", brightGreen: "#99ffe4",
    brightYellow: "#ffc799", brightBlue: "#b9aeda", brightMagenta: "#ecaad6",
    brightCyan: "#f591b2", brightWhite: "#ffffff"
  }, "#ffc799")
];

export function terminalTheme(id: TerminalTheme | undefined): TerminalThemeDefinition {
  return terminalThemes.find((theme) => theme.id === id) ?? terminalThemes[0];
}
