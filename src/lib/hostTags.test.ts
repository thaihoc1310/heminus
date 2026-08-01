import { describe, expect, it } from "vitest";
import {
  collectHostTags,
  dedupeHostTags,
  hostHasSelectedTags,
  removeHostTag,
  renameHostTag
} from "./hostTags";

describe("host tags", () => {
  it("cleans and deduplicates tags without losing their display casing", () => {
    expect(dedupeHostTags([" Production ", "production", "k8s   cluster"])).toEqual([
      "Production",
      "k8s cluster"
    ]);
  });

  it("collects a sorted tag catalogue", () => {
    expect(collectHostTags([{ tags: ["staging", "K8s"] }, { tags: ["k8s", "dev"] }])).toEqual([
      "dev",
      "K8s",
      "staging"
    ]);
  });

  it("requires a host to contain every selected filter tag", () => {
    const host = { tags: ["Production", "K8s"] };
    expect(hostHasSelectedTags(host, ["production", "k8s"])).toBe(true);
    expect(hostHasSelectedTags(host, ["production", "database"])).toBe(false);
  });

  it("renames, merges, and removes tags case-insensitively", () => {
    expect(renameHostTag(["prod", "production", "db"], "prod", "Production")).toEqual([
      "Production",
      "db"
    ]);
    expect(removeHostTag(["Production", "db"], "production")).toEqual(["db"]);
  });
});
