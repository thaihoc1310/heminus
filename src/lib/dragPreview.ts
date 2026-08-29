function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), maximum);
}

let clearActivePreview: (() => void) | null = null;

export interface NativeTabDragPreview {
  label: string;
  icon: "vault" | "folder" | "grid" | "terminal";
  background: string;
  activeBackground: string;
  foreground: string;
  border: string;
}

/**
 * Shortens a tab label to fit the drag pill.
 *
 * Both renderers draw the same pill, so they have to agree: they used to cut at
 * different lengths with different ellipses, and a long label looked different
 * in the drag image than in the floating preview.
 */
export function truncatedTabLabel(label: string): string {
  return label.length > 24 ? `${label.slice(0, 23)}…` : label;
}

function roundedRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number
): void {
  context.beginPath();
  context.roundRect(x, y, width, height, radius);
}

function drawTabIcon(
  context: CanvasRenderingContext2D,
  icon: NativeTabDragPreview["icon"],
  x: number,
  y: number
): void {
  context.save();
  context.translate(x, y);
  context.lineWidth = 1.4;
  context.lineCap = "round";
  context.lineJoin = "round";
  if (icon === "grid") {
    for (const [left, top] of [[0, 0], [7, 0], [0, 7], [7, 7]]) {
      context.strokeRect(left, top, 5, 5);
    }
  } else if (icon === "folder") {
    context.beginPath();
    context.moveTo(0, 3);
    context.lineTo(5, 3);
    context.lineTo(7, 5);
    context.lineTo(14, 5);
    context.lineTo(14, 13);
    context.lineTo(0, 13);
    context.closePath();
    context.stroke();
  } else if (icon === "vault") {
    context.strokeRect(0, 1, 14, 12);
    context.beginPath();
    context.arc(7, 7, 2.5, 0, Math.PI * 2);
    context.stroke();
  } else {
    context.strokeRect(0, 1, 14, 12);
    context.beginPath();
    context.moveTo(3, 5);
    context.lineTo(6, 7);
    context.lineTo(3, 9);
    context.moveTo(8, 9);
    context.lineTo(11, 9);
    context.stroke();
  }
  context.restore();
}

function renderNativeTabDragPreview(
  options: NativeTabDragPreview,
  displayScale: number
): HTMLCanvasElement | null {
  const width = 168;
  const height = 32;
  const backingWidth = Math.max(1, Math.round(width * displayScale));
  const backingHeight = Math.max(1, Math.round(height * displayScale));
  const canvas = document.createElement("canvas");
  canvas.width = backingWidth;
  canvas.height = backingHeight;
  const context = canvas.getContext("2d");
  if (!context) return null;
  context.scale(backingWidth / width, backingHeight / height);

  roundedRect(context, 0.5, 0.5, width - 1, height - 1, 13);
  context.fillStyle = options.background;
  context.fill();
  context.fillStyle = options.activeBackground;
  context.fill();
  context.strokeStyle = options.border;
  context.lineWidth = 1;
  context.stroke();

  context.fillStyle = options.foreground;
  context.strokeStyle = options.foreground;
  drawTabIcon(context, options.icon, 12, 9);
  context.font = "600 12px system-ui, sans-serif";
  context.textBaseline = "middle";
  const label = truncatedTabLabel(options.label);
  context.fillText(label, 36, height / 2, width - 48);
  return canvas;
}

export function createNativeTabDragPreviewDataUrl(
  options: NativeTabDragPreview
): string {
  const displayScale = Math.max(1, window.devicePixelRatio || 1);
  return renderNativeTabDragPreview(options, displayScale)?.toDataURL("image/png") ?? "";
}

/**
 * Uses the browser/WebView's native drag image so it can leave the current
 * native window. WebKitGTK applies the display scale again when handing the
 * drag icon to GTK, so expose an inverse-scaled CSS box while keeping a HiDPI
 * backing store. At 200%, an 84px box becomes the intended 168px pill and its
 * 336px backing bitmap keeps text and icons sharp.
 */
export function setNativeTabDragPreview(
  event: DragEvent,
  options: NativeTabDragPreview
): void {
  const transfer = event.dataTransfer;
  if (!transfer) return;
  clearActivePreview?.();

  const width = 168;
  const height = 32;
  const displayScale = Math.max(1, window.devicePixelRatio || 1);
  const nativeWidth = Math.max(1, Math.round(width / displayScale));
  const nativeHeight = Math.max(1, Math.round(height / displayScale));
  const backingWidth = Math.max(1, Math.round(width * displayScale));
  const backingHeight = Math.max(1, Math.round(height * displayScale));
  const canvas = document.createElement("canvas");
  canvas.width = backingWidth;
  canvas.height = backingHeight;
  canvas.style.cssText = [
    "position:fixed",
    "z-index:-1",
    "left:0",
    "top:0",
    `width:${nativeWidth}px`,
    `height:${nativeHeight}px`,
    "pointer-events:none"
  ].join(";");
  canvas.setAttribute("aria-hidden", "true");
  const context = canvas.getContext("2d");
  if (!context) return;
  context.scale(backingWidth / width, backingHeight / height);

  roundedRect(context, 0.5, 0.5, width - 1, height - 1, 13);
  context.fillStyle = options.background;
  context.fill();
  context.fillStyle = options.activeBackground;
  context.fill();
  context.strokeStyle = options.border;
  context.lineWidth = 1;
  context.stroke();

  context.fillStyle = options.foreground;
  context.strokeStyle = options.foreground;
  drawTabIcon(context, options.icon, 12, 9);
  context.font = "600 12px system-ui, sans-serif";
  context.textBaseline = "middle";
  const label = truncatedTabLabel(options.label);
  context.fillText(label, 36, height / 2, width - 48);

  document.body.append(canvas);
  transfer.setDragImage(canvas, nativeWidth / 2, nativeHeight / 2);

  const cleanup = () => {
    canvas.remove();
    document.removeEventListener("dragend", cleanup, true);
    document.removeEventListener("drop", cleanup, true);
    if (clearActivePreview === cleanup) clearActivePreview = null;
  };
  clearActivePreview = cleanup;
  document.addEventListener("dragend", cleanup, true);
  document.addEventListener("drop", cleanup, true);
}

/**
 * Uses a normal fixed DOM clone as the visual drag preview and suppresses the
 * WebView's native snapshot with a 1px canvas. WebKitGTK rasterizes native DOM
 * drag images at the display scale, which can make them appear much larger
 * than their source on HiDPI displays.
 */
export function setElementDragPreview(event: DragEvent, itemCount = 1): void {
  const transfer = event.dataTransfer;
  const source = event.currentTarget;
  if (!transfer || !(source instanceof HTMLElement)) return;

  const bounds = source.getBoundingClientRect();
  if (bounds.width <= 0 || bounds.height <= 0) return;

  clearActivePreview?.();

  const offsetX = clamp(event.clientX - bounds.left, 0, bounds.width);
  const offsetY = clamp(event.clientY - bounds.top, 0, bounds.height);
  const preview = source.cloneNode(true) as HTMLElement;
  preview.removeAttribute("id");
  preview.setAttribute("aria-hidden", "true");
  preview.classList.add("heminus-drag-preview");
  preview.classList.remove("vault-dragging");
  if (itemCount > 1) {
    preview.classList.add("multi-drag-preview");
    preview.dataset.count = String(itemCount);
  }

  const fixedStyles: Record<string, string> = {
    position: "fixed",
    zIndex: "2147483646",
    left: `${bounds.left}px`,
    top: `${bounds.top}px`,
    width: `${bounds.width}px`,
    height: `${bounds.height}px`,
    minWidth: "0",
    minHeight: "0",
    maxWidth: "none",
    maxHeight: "none",
    margin: "0",
    opacity: "0.92",
    overflow: itemCount > 1 ? "visible" : "hidden",
    pointerEvents: "none",
    transform: "none",
    transition: "none",
    boxSizing: "border-box"
  };
  for (const [property, value] of Object.entries(fixedStyles)) {
    preview.style.setProperty(
      property.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`),
      value,
      "important"
    );
  }

  // Keep this inside the viewport until dragend. WebKitGTK may ignore an
  // off-screen drag image and fall back to its oversized native snapshot.
  const nativeImage = document.createElement("canvas");
  nativeImage.width = 1;
  nativeImage.height = 1;
  nativeImage.setAttribute("aria-hidden", "true");
  nativeImage.style.cssText =
    "position:fixed;z-index:2147483645;left:0;top:0;width:1px;height:1px;background:rgba(0,0,0,.001);pointer-events:none";

  document.body.append(preview, nativeImage);
  transfer.setDragImage(nativeImage, 0, 0);

  const move = (moveEvent: DragEvent) => {
    // Some engines report 0,0 for the final synthetic drag event.
    if (moveEvent.clientX === 0 && moveEvent.clientY === 0) return;
    preview.style.setProperty("left", `${moveEvent.clientX - offsetX}px`, "important");
    preview.style.setProperty("top", `${moveEvent.clientY - offsetY}px`, "important");
  };
  // WebKitGTK does not always deliver dragend or drop when the pointer leaves
  // the window, and without a fallback the floating pill stays on top of the
  // whole app until the next drag happens to clean it up.
  let safetyTimer: number | null = window.setTimeout(() => cleanup(), 30_000);
  const cleanup = () => {
    if (safetyTimer !== null) {
      window.clearTimeout(safetyTimer);
      safetyTimer = null;
    }
    preview.remove();
    nativeImage.remove();
    document.removeEventListener("drag", move, true);
    document.removeEventListener("dragover", move, true);
    document.removeEventListener("dragend", cleanup, true);
    document.removeEventListener("drop", cleanup, true);
    document.removeEventListener("pointerup", cleanup, true);
    document.removeEventListener("mouseup", cleanup, true);
    window.removeEventListener("blur", cleanup);
    if (clearActivePreview === cleanup) clearActivePreview = null;
  };

  clearActivePreview = cleanup;
  document.addEventListener("drag", move, true);
  document.addEventListener("dragover", move, true);
  document.addEventListener("dragend", cleanup, true);
  document.addEventListener("drop", cleanup, true);
  document.addEventListener("pointerup", cleanup, true);
  document.addEventListener("mouseup", cleanup, true);
  window.addEventListener("blur", cleanup);
}
