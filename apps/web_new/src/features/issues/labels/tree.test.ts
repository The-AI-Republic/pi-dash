// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Label } from "@pidash/api-client";
import { describe, expect, it } from "vitest";
import { compareLabelOrder, filterLabelTree, findLabelByName, selectLabelTree } from "./tree.js";

function row(id: string, name: string, sort: number, parent: string | null = null): Label {
  return {
    id,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name,
    color: "#FF6900",
    parent,
    sort_order: sort,
  };
}

describe("compareLabelOrder", () => {
  it("sorts ascending by the ordering value", () => {
    expect(compareLabelOrder({ sort_order: 1 }, { sort_order: 2 })).toBeLessThan(0);
    expect(compareLabelOrder({ sort_order: 2 }, { sort_order: 1 })).toBeGreaterThan(0);
    expect(compareLabelOrder({ sort_order: 2 }, { sort_order: 2 })).toBe(0);
  });
});

describe("selectLabelTree", () => {
  it("returns an empty tree for an empty list", () => {
    expect(selectLabelTree([])).toEqual([]);
  });

  it("sorts top-level labels by the ordering value", () => {
    const tree = selectLabelTree([row("b", "B", 20000), row("a", "A", 10000)]);
    expect(tree.map((group) => group.label.name)).toEqual(["A", "B"]);
    expect(tree[0]?.children).toEqual([]);
  });

  it("nests children under their parent, sorted", () => {
    const tree = selectLabelTree([
      row("p", "Parent", 10000),
      row("c2", "Second", 30000, "p"),
      row("c1", "First", 20000, "p"),
    ]);
    expect(tree).toHaveLength(1);
    expect(tree[0]?.label.name).toBe("Parent");
    expect(tree[0]?.children.map((child) => child.name)).toEqual(["First", "Second"]);
  });

  it("renders orphans, self-parents, and grandchildren at top level", () => {
    const tree = selectLabelTree([
      row("p", "Parent", 10000),
      row("c", "Child", 20000, "p"),
      row("orphan", "Orphan", 30000, "missing"),
      row("self", "Self", 40000, "self"),
      row("grand", "Grand", 50000, "c"),
    ]);
    expect(tree.map((group) => group.label.name)).toEqual(["Parent", "Orphan", "Self", "Grand"]);
    expect(tree[0]?.children.map((child) => child.name)).toEqual(["Child"]);
  });
});

describe("filterLabelTree", () => {
  const tree = selectLabelTree([
    row("p", "Platform", 10000),
    row("c", "Crash on login", 20000, "p"),
    row("t", "Triage", 30000),
  ]);

  it("returns the whole tree for a blank query", () => {
    expect(filterLabelTree(tree, "  ")).toEqual(tree);
  });

  it("keeps a matching group with all its children", () => {
    const filtered = filterLabelTree(tree, "plat");
    expect(filtered.map((group) => group.label.name)).toEqual(["Platform"]);
    expect(filtered[0]?.children.map((child) => child.name)).toEqual(["Crash on login"]);
  });

  it("keeps only matching children under a non-matching parent", () => {
    const filtered = filterLabelTree(tree, "crash");
    expect(filtered.map((group) => group.label.name)).toEqual(["Platform"]);
    expect(filtered[0]?.children.map((child) => child.name)).toEqual(["Crash on login"]);
  });

  it("drops groups with no match", () => {
    expect(filterLabelTree(tree, "zzz")).toEqual([]);
  });
});

describe("findLabelByName", () => {
  const labels = [row("a", "Triage", 10000)];

  it("matches exact names case-insensitively", () => {
    expect(findLabelByName(labels, "triage")?.id).toBe("a");
    expect(findLabelByName(labels, "  TRIAGE  ")?.id).toBe("a");
  });

  it("matches nothing blank or missing", () => {
    expect(findLabelByName(labels, "   ")).toBe(null);
    expect(findLabelByName(labels, "Tria")).toBe(null);
    expect(findLabelByName([], "Triage")).toBe(null);
  });
});
