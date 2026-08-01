import type { Host } from "./types";

export function cleanHostTag(value: string): string {
  return value.trim().replace(/\s+/g, " ");
}

export function dedupeHostTags(values: Iterable<string>): string[] {
  const tags: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    const tag = cleanHostTag(value);
    const key = tag.toLocaleLowerCase();
    if (!tag || seen.has(key)) continue;
    seen.add(key);
    tags.push(tag);
  }
  return tags;
}

export function collectHostTags(hosts: Array<Pick<Host, "tags">>): string[] {
  return dedupeHostTags(hosts.flatMap((host) => host.tags)).sort((left, right) =>
    left.localeCompare(right, undefined, { sensitivity: "base" })
  );
}

export function hostHasSelectedTags(
  host: Pick<Host, "tags">,
  selectedTags: Iterable<string>
): boolean {
  const available = new Set(host.tags.map((tag) => cleanHostTag(tag).toLocaleLowerCase()));
  return [...selectedTags].every((tag) => available.has(cleanHostTag(tag).toLocaleLowerCase()));
}

export function renameHostTag(tags: Iterable<string>, oldTag: string, newTag: string): string[] {
  const oldKey = cleanHostTag(oldTag).toLocaleLowerCase();
  return dedupeHostTags(
    [...tags].map((tag) =>
      cleanHostTag(tag).toLocaleLowerCase() === oldKey ? newTag : tag
    )
  );
}

export function removeHostTag(tags: Iterable<string>, target: string): string[] {
  const targetKey = cleanHostTag(target).toLocaleLowerCase();
  return dedupeHostTags(
    [...tags].filter((tag) => cleanHostTag(tag).toLocaleLowerCase() !== targetKey)
  );
}
