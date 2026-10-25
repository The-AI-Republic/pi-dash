// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested page model (parity/specs/pages.md §11.7, §12.2, PAGE-084–090):
// parenthood, the expanded forest, ancestor trails with a restricted
// fallback, move legality and refusals, the delete re-home count and the
// sub-page visibility rule.

import { describe, expect, it } from "vitest";

import type { PageRow } from "@pidash/api-client";
import { makePageRow } from "../testing.js";
import {
  childrenOf,
  descendantIds,
  filterMoveTargets,
  flattenPageTree,
  hasRestrictedAncestor,
  pageAncestors,
  pageChildCount,
  pageDeleteRehomedCount,
  pageDeleteRehomedMessage,
  pageMoveRefusal,
  pageMoveRefusalMessage,
  pageMoveTargets,
  pageParentId,
  pageParentPath,
  PAGE_RESTRICTED_ANCESTOR,
  restoreDetaches,
} from "./pageNested.js";

let nextId = 0;
function id(_label: string): string {
  // Unique UUIDs so maps, cycles and the schemas behave like the server's.
  nextId += 1;
  const tail = nextId.toString(16).padStart(12, "0");
  return `123e4567-e89b-12d3-a456-42661417${tail.slice(8)}`;
}

function page(name: string, parentId: string | null, overrides: Partial<PageRow> = {}): PageRow {
  const pageId = id(name);
  return makePageRow({
    id: pageId,
    name,
    parent: parentId,
    parent_id: parentId,
    sub_pages_count: 0,
    ...overrides,
  });
}

describe("parenthood", () => {
  it("prefers the flat parent id and falls back to the container id", () => {
    const parent = id("p");
    expect(pageParentId(makePageRow({ parent: parent, parent_id: parent }))).toBe(parent);
    expect(pageParentId(makePageRow({ parent: parent }))).toBe(parent);
    expect(pageParentId(makePageRow({ parent: null, parent_id: null }))).toBeNull();
  });

  it("treats a missing child count as zero", () => {
    expect(pageChildCount(makePageRow())).toBe(0);
    expect(pageChildCount(makePageRow({ sub_pages_count: 2 }))).toBe(2);
  });

  it("partitions the pool by one parent", () => {
    const parent = page("Parent", null, { sub_pages_count: 2 });
    const first = page("First", parent.id);
    const second = page("Second", parent.id);
    const other = page("Other", null);
    expect(childrenOf([parent, first, second, other], parent.id).map((row) => row.id)).toEqual([first.id, second.id]);
  });
});

describe("flattenPageTree", () => {
  const root = page("Root", null, { sub_pages_count: 1 });
  const child = page("Child", root.id, { sub_pages_count: 1 });
  const grandchild = page("Grandchild", child.id, { sub_pages_count: 1 });
  const greatGrandchild = page("Great", grandchild.id);

  it("shows only top-level lines while every parent is collapsed", () => {
    const entries = flattenPageTree([root], { expanded: new Set(), children: new Map(), pending: new Set() });
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ depth: 0, hasChildren: true, expanded: false, loading: false });
  });

  it("indents children recursively under expanded parents", () => {
    const children = new Map<string, readonly PageRow[]>([
      [root.id, [child]],
      [child.id, [grandchild]],
      [grandchild.id, [greatGrandchild]],
    ]);
    const entries = flattenPageTree([root], {
      expanded: new Set([root.id, child.id, grandchild.id]),
      children,
      pending: new Set(),
    });
    expect(entries.map((entry) => [entry.page.name, entry.depth])).toEqual([
      ["Root", 0],
      ["Child", 1],
      ["Grandchild", 2],
      ["Great", 3],
    ]);
    expect(entries[3]?.hasChildren).toBe(false);
  });

  it("skips a collapsed parent's loaded children without dropping the control", () => {
    const entries = flattenPageTree([root], {
      expanded: new Set(),
      children: new Map([[root.id, [child]]]),
      pending: new Set(),
    });
    expect(entries).toHaveLength(1);
    expect(entries[0]?.hasChildren).toBe(true);
  });

  it("reports a parent whose children read is in flight", () => {
    const entries = flattenPageTree([root], {
      expanded: new Set([root.id]),
      children: new Map(),
      pending: new Set([root.id]),
    });
    expect(entries[0]).toMatchObject({ hasChildren: true, expanded: true, loading: true });
  });

  it("survives a malformed cycle in server data", () => {
    const loopA = page("Loop A", null, { sub_pages_count: 1 });
    const loopB = page("Loop B", loopA.id, { sub_pages_count: 1 });
    const cyclicA = { ...loopA, parent: loopB.id, parent_id: loopB.id };
    const entries = flattenPageTree([cyclicA], {
      expanded: new Set([loopA.id, loopB.id]),
      children: new Map([
        [loopA.id, [loopB]],
        [loopB.id, [cyclicA]],
      ]),
      pending: new Set(),
    });
    expect(entries.map((entry) => entry.page.name)).toEqual(["Loop A", "Loop B"]);
  });
});

describe("ancestry and breadcrumbs", () => {
  const top = page("Handbook", null, { sub_pages_count: 1 });
  const middle = page("Policies", top.id, { sub_pages_count: 1 });
  const leaf = page("Leave", middle.id);
  const pool = [top, middle, leaf];

  it("lists every ancestor top-down", () => {
    expect(pageAncestors(pool, leaf.id).map((entry) => [entry.name, entry.visible])).toEqual([
      ["Handbook", true],
      ["Policies", true],
    ]);
    expect(pageAncestors(pool, top.id)).toEqual([]);
  });

  it("renders an ancestor the caller cannot see as the restricted fallback", () => {
    // The child is visible; the middle row never reached the caller.
    const orphan = page("Leave", middle.id);
    const ancestors = pageAncestors([top, orphan], orphan.id);
    expect(ancestors).toEqual([{ id: middle.id, name: PAGE_RESTRICTED_ANCESTOR, visible: false }]);
    expect(hasRestrictedAncestor(ancestors)).toBe(true);
  });

  it("stops at a gap instead of inventing higher ancestors", () => {
    const orphan = page("Leave", middle.id);
    expect(pageAncestors([orphan], orphan.id)).toEqual([
      { id: middle.id, name: PAGE_RESTRICTED_ANCESTOR, visible: false },
    ]);
  });

  it("survives a malformed ancestry cycle", () => {
    const a = page("A", null, { sub_pages_count: 1 });
    const b = { ...page("B", a.id), parent: a.id, parent_id: a.id };
    expect(pageAncestors([{ ...a, parent: b.id, parent_id: b.id }, b], a.id)).toHaveLength(1);
  });

  it("treats a page absent from the pool as having no resolvable ancestors", () => {
    // The sidebar sibling list can be narrower than the caller's forest.
    expect(pageAncestors([top], "123e4567-e89b-12d3-a456-426614174999")).toEqual([]);
  });

  it("builds the parent path used as a search result's secondary line", () => {
    expect(pageParentPath(pool, leaf.id)).toBe("Handbook / Policies");
    expect(pageParentPath(pool, top.id)).toBeNull();
  });
});

describe("move rules", () => {
  const top = page("Handbook", null, { sub_pages_count: 1 });
  const middle = page("Policies", top.id, { sub_pages_count: 1 });
  const leaf = page("Leave", middle.id);
  const other = page("Other", null);
  const shelved = page("Shelved", null, { archived_at: "2026-10-20" });
  const pool = [top, middle, leaf, other, shelved];

  it("collects every descendant, not just direct children", () => {
    expect([...descendantIds(pool, top.id)].sort()).toEqual([middle.id, leaf.id].sort());
    expect([...descendantIds(pool, leaf.id)]).toEqual([]);
  });

  it("allows another live page and the top level", () => {
    expect(pageMoveRefusal(pool, leaf.id, other.id)).toBeNull();
    expect(pageMoveRefusal(pool, leaf.id, null)).toBeNull();
  });

  it("refuses itself, a descendant and an archived target with a message each", () => {
    expect(pageMoveRefusal(pool, middle.id, middle.id)).toBe("self");
    expect(pageMoveRefusal(pool, top.id, leaf.id)).toBe("descendant");
    expect(pageMoveRefusal(pool, leaf.id, shelved.id)).toBe("archived");
    expect(pageMoveRefusal(pool, leaf.id, "123e4567-e89b-12d3-a456-426614174999")).toBe("missing");
    for (const refusal of ["self", "descendant", "archived", "missing"] as const) {
      expect(pageMoveRefusalMessage(refusal)).toMatch(/page/i);
    }
  });

  it("offers every live page except the page and its subtree", () => {
    expect(pageMoveTargets(pool, top.id).map((row) => row.name)).toEqual(["Other"]);
    expect(pageMoveTargets(pool, leaf.id).map((row) => row.name)).toEqual(["Handbook", "Policies", "Other"]);
  });

  it("searches the picker by title and by parent path", () => {
    const targets = pageMoveTargets(pool, other.id);
    expect(filterMoveTargets(targets, pool, "").map((row) => row.name)).toEqual(["Handbook", "Policies", "Leave"]);
    // A parent's name also finds its children: their path carries it.
    expect(filterMoveTargets(targets, pool, "hand").map((row) => row.name)).toEqual(["Handbook", "Policies", "Leave"]);
    // Only the deepest page carries the full trail.
    expect(filterMoveTargets(targets, pool, "handbook / policies").map((row) => row.name)).toEqual(["Leave"]);
  });
});

describe("archive, restore and delete", () => {
  const parent = page("Parent", null, { sub_pages_count: 1 });
  const child = page("Child", parent.id);

  it("detects a restore under a still-archived parent", () => {
    const shelvedParent = { ...parent, archived_at: "2026-10-20" };
    expect(restoreDetaches([shelvedParent, child], child)).toBe(true);
    expect(restoreDetaches([parent, child], child)).toBe(false);
    expect(restoreDetaches([parent, parent], parent)).toBe(false);
    // The parent can be missing from the loaded tab, so no stale notice.
    expect(restoreDetaches([child], child)).toBe(false);
  });

  it("states how many direct children a delete re-homes", () => {
    expect(pageDeleteRehomedCount(parent)).toBe(1);
    expect(pageDeleteRehomedCount(child)).toBe(0);
    expect(pageDeleteRehomedMessage(0)).toBeNull();
    expect(pageDeleteRehomedMessage(1)).toBe("1 sub-page will move to the top level.");
    expect(pageDeleteRehomedMessage(4)).toBe("4 sub-pages will move to the top level.");
  });
});
