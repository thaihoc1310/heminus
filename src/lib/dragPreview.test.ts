/** @vitest-environment jsdom */

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createNativeTabDragPreviewDataUrl,
  setElementDragPreview,
  setNativeTabDragPreview
} from "./dragPreview";

describe("drag preview", () => {
  afterEach(() => {
    document.dispatchEvent(new Event("dragend", { bubbles: true }));
    document.body.replaceChildren();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("compensates the native tab preview for a 200% display scale", () => {
    vi.stubGlobal("devicePixelRatio", 2);
    const context = {
      beginPath: vi.fn(),
      roundRect: vi.fn(),
      fill: vi.fn(),
      stroke: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      translate: vi.fn(),
      strokeRect: vi.fn(),
      arc: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      closePath: vi.fn(),
      fillText: vi.fn(),
      scale: vi.fn()
    };
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
      context as unknown as CanvasRenderingContext2D
    );
    const setDragImage = vi.fn();

    setNativeTabDragPreview(
      { dataTransfer: { setDragImage } } as unknown as DragEvent,
      {
        label: "vm-1",
        icon: "terminal",
        background: "#101114",
        activeBackground: "#272a30",
        foreground: "#f5f5f5",
        border: "#3a3d44"
      }
    );

    const canvas = document.querySelector("canvas");
    expect(canvas?.width).toBe(336);
    expect(canvas?.height).toBe(64);
    expect(canvas?.style.width).toBe("84px");
    expect(canvas?.style.height).toBe("16px");
    expect(context.scale).toHaveBeenCalledWith(2, 2);
    expect(setDragImage).toHaveBeenCalledWith(canvas, 42, 8);
  });

  it("renders a PNG data URL for the desktop native drag API", () => {
    vi.stubGlobal("devicePixelRatio", 1.5);
    const context = {
      beginPath: vi.fn(),
      roundRect: vi.fn(),
      fill: vi.fn(),
      stroke: vi.fn(),
      save: vi.fn(),
      restore: vi.fn(),
      translate: vi.fn(),
      strokeRect: vi.fn(),
      arc: vi.fn(),
      moveTo: vi.fn(),
      lineTo: vi.fn(),
      closePath: vi.fn(),
      fillText: vi.fn(),
      scale: vi.fn()
    };
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
      context as unknown as CanvasRenderingContext2D
    );
    vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockReturnValue(
      "data:image/png;base64,preview"
    );

    const result = createNativeTabDragPreviewDataUrl({
      label: "Local Terminal",
      icon: "terminal",
      background: "#101114",
      activeBackground: "#272a30",
      foreground: "#f5f5f5",
      border: "#3a3d44"
    });

    expect(result).toBe("data:image/png;base64,preview");
    expect(context.scale).toHaveBeenCalledWith(1.5, 1.5);
  });

  it("keeps a fixed DOM preview at the exact source size", () => {
    const source = document.createElement("article");
    source.textContent = "vm-1";
    document.body.appendChild(source);
    vi.spyOn(source, "getBoundingClientRect").mockReturnValue({
      x: 100,
      y: 50,
      left: 100,
      top: 50,
      right: 420,
      bottom: 118,
      width: 320,
      height: 68,
      toJSON: () => ({})
    });
    const setDragImage = vi.fn();

    setElementDragPreview({
      currentTarget: source,
      dataTransfer: { setDragImage },
      clientX: 140,
      clientY: 70
    } as unknown as DragEvent);

    const preview = document.querySelector<HTMLElement>(".heminus-drag-preview");
    expect(preview?.style.width).toBe("320px");
    expect(preview?.style.height).toBe("68px");
    expect(preview?.style.left).toBe("100px");
    expect(preview?.style.top).toBe("50px");

    expect(setDragImage).toHaveBeenCalledOnce();
    const [nativeImage, offsetX, offsetY] = setDragImage.mock.calls[0];
    expect(nativeImage).toBeInstanceOf(HTMLCanvasElement);
    expect(nativeImage.width).toBe(1);
    expect(nativeImage.height).toBe(1);
    expect([offsetX, offsetY]).toEqual([0, 0]);

    document.dispatchEvent(
      new MouseEvent("dragover", {
        bubbles: true,
        clientX: 200,
        clientY: 120
      })
    );
    expect(preview?.style.left).toBe("160px");
    expect(preview?.style.top).toBe("100px");

    document.dispatchEvent(new Event("drop", { bubbles: true }));
    expect(preview?.isConnected).toBe(false);
    expect(nativeImage.isConnected).toBe(false);
  });
});
