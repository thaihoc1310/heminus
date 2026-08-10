export const topTabDragThreshold = 8;

export function hasPassedTopTabDragThreshold(
  startX: number,
  startY: number,
  currentX: number,
  currentY: number
): boolean {
  return Math.hypot(currentX - startX, currentY - startY) >= topTabDragThreshold;
}
