import { describe, expect, it } from "vitest";
import {
  buildGroupIndex,
  buildGroupRows,
  collectGroupSubtree,
  countHostsPerGroup
} from "./hostGroups";
import type { Host, VaultGroup } from "./types";

function group(id: string, name: string, parent: string | null = null): VaultGroup {
  return {
    id,
    name,
    parent_id: parent,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z"
  };
}

function host(id: string, groupId: string | null): Host {
  return {
    id,
    label: `Host ${id}`,
    address: "10.0.0.1",
    port: 22,
    username: "deploy",
    group_name: null,
    group_id: groupId,
    tags: [],
    color: "amber",
    identity_id: null,
    jump_host_ids: [],
    environment: [],
    proxy: null,
    terminal_theme: "heminus_dark",
    terminal_font_size: 14,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z"
  } as Host;
}

const groups = [
  group("prod", "Production"),
  group("eu", "EU", "prod"),
  group("us", "US", "prod"),
  group("edge", "Edge", "eu"),
  group("dev", "Development")
];

describe("buildGroupRows", () => {
  it("lists groups depth-first with their display paths", () => {
    const rows = buildGroupRows(groups);
    expect(rows.map((row) => [row.group.id, row.depth, row.path])).toEqual([
      ["dev", 0, "Development"],
      ["prod", 0, "Production"],
      ["eu", 1, "Production / EU"],
      ["edge", 2, "Production / EU / Edge"],
      ["us", 1, "Production / US"]
    ]);
  });

  it("still surfaces a group whose parent no longer exists", () => {
    const rows = buildGroupRows([group("orphan", "Orphan", "missing")]);
    expect(rows).toEqual([
      { group: rows[0].group, depth: 0, path: "Orphan" }
    ]);
  });
});

describe("collectGroupSubtree", () => {
  it("includes the group and everything nested under it", () => {
    const subtree = collectGroupSubtree("prod", buildGroupIndex(groups));
    expect([...subtree].sort()).toEqual(["edge", "eu", "prod", "us"]);
  });

  it("returns just the group when it has no children", () => {
    expect([...collectGroupSubtree("edge", buildGroupIndex(groups))]).toEqual(["edge"]);
  });

  it("terminates on a parent cycle in stored data", () => {
    const cyclic = [group("a", "A", "b"), group("b", "B", "a")];
    expect([...collectGroupSubtree("a", buildGroupIndex(cyclic)).values()].sort()).toEqual([
      "a",
      "b"
    ]);
  });
});

describe("countHostsPerGroup", () => {
  it("counts hosts in nested groups against every ancestor", () => {
    const counts = countHostsPerGroup(groups, [
      host("1", "edge"),
      host("2", "eu"),
      host("3", "us"),
      host("4", "dev"),
      host("5", null)
    ]);

    expect(counts.get("prod")).toBe(3);
    expect(counts.get("eu")).toBe(2);
    expect(counts.get("edge")).toBe(1);
    expect(counts.get("us")).toBe(1);
    expect(counts.get("dev")).toBe(1);
  });

  it("ignores hosts pointing at a group that is gone", () => {
    const counts = countHostsPerGroup(groups, [host("1", "deleted")]);
    expect(counts.size).toBe(0);
  });

  it("does not loop forever when groups reference each other", () => {
    const cyclic = [group("a", "A", "b"), group("b", "B", "a")];
    const counts = countHostsPerGroup(cyclic, [host("1", "a")]);
    expect(counts.get("a")).toBe(1);
    expect(counts.get("b")).toBe(1);
  });
});
