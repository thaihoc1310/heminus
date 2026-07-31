export interface SelectionRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function selectionRect(
  startX: number,
  startY: number,
  currentX: number,
  currentY: number
): SelectionRect {
  return {
    left: Math.min(startX, currentX),
    top: Math.min(startY, currentY),
    right: Math.max(startX, currentX),
    bottom: Math.max(startY, currentY)
  };
}

export function rectanglesIntersect(left: SelectionRect, right: SelectionRect): boolean {
  return left.left <= right.right && left.right >= right.left &&
    left.top <= right.bottom && left.bottom >= right.top;
}

export function beginMarqueeSelection(
  event: PointerEvent,
  container: HTMLElement,
  currentSelection: Iterable<string>,
  onselectionchange: (ids: string[]) => void
): boolean {
  if (
    event.button !== 0 ||
    (event.target as HTMLElement).closest("[data-select-id], button, input, textarea, select, a")
  ) return false;

  event.preventDefault();
  const startX = event.clientX;
  const startY = event.clientY;
  const base = new Set(event.ctrlKey || event.metaKey ? currentSelection : []);
  const overlay = document.createElement("div");
  overlay.className = "selection-marquee";
  overlay.setAttribute("aria-hidden", "true");
  document.body.append(overlay);

  const update = (pointer: PointerEvent) => {
    const rect = selectionRect(startX, startY, pointer.clientX, pointer.clientY);
    overlay.style.left = `${rect.left}px`;
    overlay.style.top = `${rect.top}px`;
    overlay.style.width = `${rect.right - rect.left}px`;
    overlay.style.height = `${rect.bottom - rect.top}px`;
    const ids = new Set(base);
    for (const item of container.querySelectorAll<HTMLElement>("[data-select-id]")) {
      const bounds = item.getBoundingClientRect();
      if (rectanglesIntersect(rect, {
        left: bounds.left,
        top: bounds.top,
        right: bounds.right,
        bottom: bounds.bottom
      })) ids.add(item.dataset.selectId ?? "");
    }
    ids.delete("");
    onselectionchange([...ids]);
  };
  const finish = () => {
    overlay.remove();
    window.removeEventListener("pointermove", update);
    window.removeEventListener("pointerup", finish);
    window.removeEventListener("pointercancel", finish);
  };
  window.addEventListener("pointermove", update);
  window.addEventListener("pointerup", finish, { once: true });
  window.addEventListener("pointercancel", finish, { once: true });
  update(event);
  return true;
}
