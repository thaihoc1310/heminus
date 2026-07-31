export function humanFileSize(size: number | null): string {
  if (size === null) return "—";
  if (size < 1024) return `${size} B`;
  if (size < 1024 ** 2) return `${(size / 1024).toFixed(1)} KB`;
  if (size < 1024 ** 3) return `${(size / 1024 ** 2).toFixed(1)} MB`;
  return `${(size / 1024 ** 3).toFixed(1)} GB`;
}

export function matchesSearch(values: Array<string | null | undefined>, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase("en");
  if (!needle) return true;
  return values
    .filter((value): value is string => typeof value === "string")
    .join(" ")
    .toLocaleLowerCase("en")
    .includes(needle);
}
