import type { Host, VaultGroup } from "./types";

export interface GroupRow {
  group: VaultGroup;
  depth: number;
  path: string;
}

/**
 * Groups indexed by parent, each child list sorted by name.
 *
 * Everything else here walks this instead of re-scanning the whole group list,
 * which is what made the vault view quadratic in the number of groups.
 */
export function buildGroupIndex(
  groups: VaultGroup[]
): Map<string | null, VaultGroup[]> {
  const index = new Map<string | null, VaultGroup[]>();
  for (const group of groups) {
    const parent = group.parent_id ?? null;
    const siblings = index.get(parent);
    if (siblings) siblings.push(group);
    else index.set(parent, [group]);
  }
  for (const siblings of index.values()) {
    siblings.sort((left, right) => left.name.localeCompare(right.name));
  }
  return index;
}

/** Every group in tree order, with its depth and display path. */
export function buildGroupRows(
  groups: VaultGroup[],
  index = buildGroupIndex(groups)
): GroupRow[] {
  const rows: GroupRow[] = [];
  const visited = new Set<string>();
  const append = (parentId: string | null, depth: number, prefix: string) => {
    for (const group of index.get(parentId) ?? []) {
      if (visited.has(group.id)) continue;
      visited.add(group.id);
      const path = prefix ? `${prefix} / ${group.name}` : group.name;
      rows.push({ group, depth, path });
      append(group.id, depth + 1, path);
    }
  };
  append(null, 0, "");
  // A group whose parent was deleted out from under it still has to appear.
  for (const group of groups) {
    if (!visited.has(group.id)) rows.push({ group, depth: 0, path: group.name });
  }
  return rows;
}

/** The group plus everything nested beneath it. */
export function collectGroupSubtree(
  id: string,
  index: Map<string | null, VaultGroup[]>
): Set<string> {
  const ids = new Set([id]);
  const pending = [id];
  while (pending.length > 0) {
    const current = pending.pop() as string;
    for (const child of index.get(current) ?? []) {
      // Guards against a parent cycle in stored data.
      if (ids.has(child.id)) continue;
      ids.add(child.id);
      pending.push(child.id);
    }
  }
  return ids;
}

/**
 * How many hosts sit in each group, counting nested groups.
 *
 * Computed for every group in one pass: each host is walked up to the root
 * once, rather than every group re-scanning every host.
 */
export function countHostsPerGroup(
  groups: VaultGroup[],
  hosts: Host[]
): Map<string, number> {
  const parents = new Map<string, string | null>();
  for (const group of groups) parents.set(group.id, group.parent_id ?? null);

  const counts = new Map<string, number>();
  for (const host of hosts) {
    let current = host.group_id ?? null;
    const seen = new Set<string>();
    while (current && parents.has(current) && !seen.has(current)) {
      seen.add(current);
      counts.set(current, (counts.get(current) ?? 0) + 1);
      current = parents.get(current) ?? null;
    }
  }
  return counts;
}
