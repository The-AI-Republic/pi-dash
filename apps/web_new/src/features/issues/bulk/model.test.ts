// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  extendSelection,
  groupSelectionState,
  moveActive,
  pruneSelection,
  toggleGroup,
  toggleItem,
  visibleBulkFields,
  type SelectionItem,
} from "./model.js";

const ITEMS: SelectionItem[] = [
  { id: "a", groupKey: "todo" },
  { id: "b", groupKey: "todo" },
  { id: "c", groupKey: "done" },
  { id: "d", groupKey: "done" },
];

describe("bulk selection model", () => {
  it("toggles one row without shift", () => {
    expect([...toggleItem(new Set(), ITEMS, "a", null, false)]).toEqual(["a"]);
    expect([...toggleItem(new Set(["a"]), ITEMS, "a", "a", false)]).toEqual([]);
  });

  it("selects forward and backward ranges from the anchor across groups", () => {
    expect([...toggleItem(new Set(["a"]), ITEMS, "c", "a", true)]).toEqual(["a", "b", "c"]);
    expect(new Set(toggleItem(new Set(["d"]), ITEMS, "b", "d", true))).toEqual(new Set(["b", "c", "d"]));
  });

  it("falls back to a single add when the anchor left the list", () => {
    expect([...toggleItem(new Set(["b"]), ITEMS, "d", "gone", true)]).toEqual(["b", "d"]);
  });

  it("reports tri-state per group and toggles the whole group", () => {
    expect(groupSelectionState(new Set(), ITEMS, "todo")).toBe("empty");
    expect(groupSelectionState(new Set(["a"]), ITEMS, "todo")).toBe("partial");
    expect(groupSelectionState(new Set(["a", "b"]), ITEMS, "todo")).toBe("complete");
    expect(groupSelectionState(new Set(["a"]), ITEMS, "missing")).toBe("empty");

    expect([...toggleGroup(new Set(["a"]), ITEMS, "todo")]).toEqual(["a", "b"]);
    expect([...toggleGroup(new Set(["a", "b"]), ITEMS, "todo")]).toEqual([]);
  });

  it("prunes ids that leave the list", () => {
    expect([...pruneSelection(new Set(["a", "gone"]), ITEMS)]).toEqual(["a"]);
  });

  it("moves the active row and wraps at the edges by staying put", () => {
    expect(moveActive(null, ITEMS, "down")).toBe("a");
    expect(moveActive(null, ITEMS, "up")).toBe("d");
    expect(moveActive("gone", ITEMS, "up")).toBe("d");
    expect(moveActive("b", ITEMS, "down")).toBe("c");
    expect(moveActive("d", ITEMS, "down")).toBe("d");
    expect(moveActive("a", ITEMS, "up")).toBe("a");
    expect(moveActive("gone", ITEMS, "down")).toBe("a");
    expect(moveActive(null, [], "down")).toBeNull();
  });

  it("extends selection by the adjacent row in the arrow direction", () => {
    expect([...extendSelection(new Set(["a"]), "a", ITEMS, "down")]).toEqual(["a", "b"]);
    expect([...extendSelection(new Set(["b"]), "b", ITEMS, "up")]).toEqual(["b", "a"]);
    expect([...extendSelection(new Set(), null, [], "down")]).toEqual([]);
  });

  it("hides cycle, module and estimate fields per project flags", () => {
    expect(visibleBulkFields({ hideCycle: true, hideModule: false, hideEstimate: false })).not.toContain("cycle_id");
    expect(visibleBulkFields({ hideCycle: false, hideModule: true, hideEstimate: false })).not.toContain("module_ids");
    expect(visibleBulkFields({ hideCycle: false, hideModule: false, hideEstimate: true })).not.toContain(
      "estimate_point"
    );
    expect(visibleBulkFields({ hideCycle: true, hideModule: true, hideEstimate: true })).toEqual([
      "state_id",
      "priority",
      "assignee_ids",
      "label_ids",
      "start_date",
      "target_date",
    ]);
  });
});
