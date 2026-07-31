export type CollectionView = "grid" | "list";
export type CollectionSort = "az" | "za" | "newest" | "oldest";

export function compareCollectionItems(
  leftLabel: string,
  rightLabel: string,
  leftCreatedAt: string | number,
  rightCreatedAt: string | number,
  sort: CollectionSort
): number {
  if (sort === "az" || sort === "za") {
    const comparison = leftLabel.localeCompare(rightLabel, undefined, {
      numeric: true,
      sensitivity: "base"
    });
    return sort === "az" ? comparison : -comparison;
  }
  const leftTime =
    typeof leftCreatedAt === "number" ? leftCreatedAt : Date.parse(leftCreatedAt) || 0;
  const rightTime =
    typeof rightCreatedAt === "number" ? rightCreatedAt : Date.parse(rightCreatedAt) || 0;
  return sort === "newest" ? rightTime - leftTime : leftTime - rightTime;
}
