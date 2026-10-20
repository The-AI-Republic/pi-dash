// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Label } from "@pidash/api-client";
import { describe, expect, it } from "vitest";
import {
  effectiveLabelChildren,
  effectiveLabelParent,
  LABEL_ORDER_STEP,
  neighborLabelOrder,
  readLabelDropSpans,
  resolveLabelChildSlot,
  resolveLabelDrop,
  resolveLabelInto,
  resolveLabelSlot,
  type LabelDropSpan,
} from "./drops.js";

function row(id: string, sort: number, parent: string | null = null): Label {
  return {
    id,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name: id,
    color: "#FF6900",
    parent,
    sort_order: sort,
  };
}

function span(id: string, top: number, nestable = true, parentId: string | null = null): LabelDropSpan {
  return { id, top, bottom: top + 40, left: 0, right: 400, nestable, parentId };
}

describe("resolveLabelSlot", () => {
  const spans = [span("a", 0), span("b", 40), span("c", 80)];

  it("resolves nothing for an empty list", () => {
    expect(resolveLabelSlot([], 10, "a")).toBe(null);
  });

  it("resolves above the row whose middle the pointer has not reached", () => {
    expect(resolveLabelSlot(spans, 5, "x")).toEqual({ id: "a", after: false });
    expect(resolveLabelSlot(spans, 45, "x")).toEqual({ id: "b", after: false });
  });

  it("resolves past the last row at the bottom", () => {
    expect(resolveLabelSlot(spans, 500, "x")).toEqual({ id: "c", after: true });
  });

  it("resolves nothing over the dragged row or past it at the tail", () => {
    expect(resolveLabelSlot(spans, 45, "b")).toBe(null);
    expect(resolveLabelSlot(spans, 500, "c")).toBe(null);
  });
});

describe("resolveLabelInto", () => {
  const spans = [span("a", 0), span("b", 40, false)];

  it("matches the middle band of a top-level row", () => {
    expect(resolveLabelInto(spans, 200, 20, "x")).toBe("a");
  });

  it("skips child rows, the dragged row, and edge releases", () => {
    expect(resolveLabelInto(spans, 200, 60, "x")).toBe(null);
    expect(resolveLabelInto(spans, 200, 20, "a")).toBe(null);
    expect(resolveLabelInto(spans, 10, 20, "x")).toBe(null);
    expect(resolveLabelInto(spans, 390, 20, "x")).toBe(null);
    expect(resolveLabelInto(spans, 200, 2, "x")).toBe(null);
    expect(resolveLabelInto(spans, 200, 200, "x")).toBe(null);
  });

  it("never matches degenerate rows", () => {
    const flat = [{ ...span("a", 0), left: 50, right: 50 }];
    expect(resolveLabelInto(flat, 50, 20, "x")).toBe(null);
    const thin = [{ ...span("a", 0), top: 20, bottom: 20 }];
    expect(resolveLabelInto(thin, 200, 20, "x")).toBe(null);
  });
});

describe("resolveLabelChildSlot", () => {
  const spans = [span("a", 0), span("b", 40, false, "a"), span("c", 80, false, "a"), span("d", 120)];

  it("previews a slot above the child under the pointer", () => {
    expect(resolveLabelChildSlot(spans, 200, 42, "x")).toEqual({ id: "b", after: false });
    expect(resolveLabelChildSlot(spans, 200, 75, "x")).toEqual({ id: "b", after: false });
  });

  it("previews a slot below the last child's lower half", () => {
    expect(resolveLabelChildSlot(spans, 200, 95, "x")).toEqual({ id: "c", after: false });
    expect(resolveLabelChildSlot(spans, 200, 115, "x")).toEqual({ id: "c", after: true });
    expect(resolveLabelChildSlot(spans, 200, 100, "x")).toEqual({ id: "c", after: false });
  });

  it("never falls through a child row onto a top-level row", () => {
    expect(resolveLabelChildSlot(spans, 200, 45, "x")).not.toBe(null);
    expect(resolveLabelChildSlot(spans, 200, 140, "x")).toBe(null);
  });

  it("skips top-level rows, the dragged row, and points outside the row", () => {
    expect(resolveLabelChildSlot(spans, 200, 20, "x")).toBe(null);
    expect(resolveLabelChildSlot(spans, 200, 60, "b")).toBe(null);
    expect(resolveLabelChildSlot(spans, -1, 60, "x")).toBe(null);
    expect(resolveLabelChildSlot(spans, 401, 60, "x")).toBe(null);
    expect(resolveLabelChildSlot(spans, 200, 200, "x")).toBe(null);
  });
});

describe("readLabelDropSpans", () => {
  it("measures every slot with its id, nestability and parent", () => {
    const root = document.createElement("ul");
    root.innerHTML = `
      <li><div data-label-slot data-label-id="a" data-nestable="true"></div></li>
      <li><div data-label-slot data-label-id="b" data-nestable="false" data-label-parent="a"></div></li>
      <li><div data-label-slot></div></li>
    `;
    const rects: Record<string, [number, number, number, number]> = {
      a: [0, 10, 100, 40],
      b: [10, 50, 100, 80],
    };
    const original = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement): DOMRect {
      const [left, top, right, bottom] = rects[this.dataset["labelId"] ?? ""] ?? [0, 0, 0, 0];
      return { left, top, right, bottom, x: left, y: top, width: right - left, height: bottom - top } as DOMRect;
    };
    try {
      expect(readLabelDropSpans(root)).toEqual([
        { id: "a", top: 10, bottom: 40, left: 0, right: 100, nestable: true, parentId: null },
        { id: "b", top: 50, bottom: 80, left: 10, right: 100, nestable: false, parentId: "a" },
      ]);
    } finally {
      HTMLElement.prototype.getBoundingClientRect = original;
    }
  });

  it("measures nothing without a root", () => {
    expect(readLabelDropSpans(null)).toEqual([]);
    expect(readLabelDropSpans(undefined)).toEqual([]);
  });
});

describe("effectiveLabelParent", () => {
  it("keeps top-level and missing links at top level", () => {
    const labels = [row("a", 10000), row("orphan", 20000, "missing"), row("self", 30000, "self")];
    expect(effectiveLabelParent(labels, labels[0]!)).toBe(null);
    expect(effectiveLabelParent(labels, labels[1]!)).toBe(null);
    expect(effectiveLabelParent(labels, labels[2]!)).toBe(null);
  });

  it("keeps children under a top-level parent", () => {
    const labels = [row("p", 10000), row("c", 20000, "p")];
    expect(effectiveLabelParent(labels, labels[1]!)).toBe("p");
  });

  it("flattens links that would nest deeper than two levels", () => {
    const labels = [row("p", 10000), row("c", 20000, "p"), row("g", 30000, "c")];
    expect(effectiveLabelParent(labels, labels[2]!)).toBe(null);
  });

  it("hangs rows under orphan-top and self-parented rows", () => {
    const labels = [
      row("orphan", 10000, "missing"),
      row("kid", 20000, "orphan"),
      row("self", 30000, "self"),
      row("kid2", 40000, "self"),
    ];
    expect(effectiveLabelParent(labels, labels[1]!)).toBe("orphan");
    expect(effectiveLabelParent(labels, labels[3]!)).toBe("self");
  });
});

describe("effectiveLabelChildren", () => {
  it("lists sorted children, never the row itself", () => {
    const labels = [
      row("p", 10000),
      row("c2", 30000, "p"),
      row("c1", 20000, "p"),
      row("t", 40000),
      row("self", 50000, "self"),
    ];
    expect(effectiveLabelChildren(labels, "p").map((child) => child.id)).toEqual(["c1", "c2"]);
    expect(effectiveLabelChildren(labels, "t")).toEqual([]);
    expect(effectiveLabelChildren(labels, "self")).toEqual([]);
  });
});

describe("neighborLabelOrder", () => {
  const rest = [row("a", 10000), row("b", 20000), row("c", 30000)];

  it("halves the next order at the head", () => {
    expect(neighborLabelOrder(rest, 0)).toBe(5000);
  });

  it("midpoints inside the list", () => {
    expect(neighborLabelOrder(rest, 1)).toBe(15000);
    expect(neighborLabelOrder(rest, 2)).toBe(25000);
  });

  it("steps past the tail", () => {
    expect(neighborLabelOrder(rest, 3)).toBe(30000 + LABEL_ORDER_STEP);
  });

  it("leaves the order to the server for an empty destination", () => {
    expect(neighborLabelOrder([], 0)).toBe(null);
  });
});

describe("resolveLabelDrop", () => {
  const flat = [row("a", 10000), row("b", 20000), row("c", 30000)];

  it("refuses unknown sources and empty drops", () => {
    expect(resolveLabelDrop({ labels: flat, sourceId: "ghost", slot: null, into: null })).toBe(null);
    expect(resolveLabelDrop({ labels: flat, sourceId: "a", slot: null, into: null })).toBe(null);
  });

  it("reorders above the first row with half the next order", () => {
    expect(resolveLabelDrop({ labels: flat, sourceId: "c", slot: { id: "a", after: false }, into: null })).toEqual({
      parent: null,
      sortOrder: 5000,
    });
  });

  it("reorders between rows with the midpoint", () => {
    expect(resolveLabelDrop({ labels: flat, sourceId: "a", slot: { id: "c", after: false }, into: null })).toEqual({
      parent: null,
      sortOrder: 25000,
    });
  });

  it("reorders past the last row with the step", () => {
    expect(resolveLabelDrop({ labels: flat, sourceId: "a", slot: { id: "c", after: true }, into: null })).toEqual({
      parent: null,
      sortOrder: 30000 + LABEL_ORDER_STEP,
    });
  });

  it("skips drops with no movement inside the same parent", () => {
    expect(resolveLabelDrop({ labels: flat, sourceId: "b", slot: { id: "c", after: false }, into: null })).toBe(null);
    expect(resolveLabelDrop({ labels: flat, sourceId: "b", slot: { id: "b", after: false }, into: null })).toBe(null);
    expect(resolveLabelDrop({ labels: flat, sourceId: "b", slot: { id: "ghost", after: false }, into: null })).toBe(
      null
    );
  });

  it("nests onto a top-level label, leaving the order to the server when empty", () => {
    expect(resolveLabelDrop({ labels: flat, sourceId: "b", slot: null, into: "a" })).toEqual({
      parent: "a",
    });
  });

  it("appends past the last child when nesting into a group", () => {
    const labels = [...flat, row("kid", 12000, "a")];
    expect(resolveLabelDrop({ labels, sourceId: "b", slot: null, into: "a" })).toEqual({
      parent: "a",
      sortOrder: 12000 + LABEL_ORDER_STEP,
    });
  });

  it("lands drops onto a child as its sibling", () => {
    const labels = [...flat, row("kid", 12000, "a")];
    expect(resolveLabelDrop({ labels, sourceId: "c", slot: null, into: "kid" })).toEqual({
      parent: "a",
      sortOrder: 6000,
    });
  });

  it("reorders a group above instead of nesting it", () => {
    const labels = [...flat, row("kid", 12000, "c")];
    expect(resolveLabelDrop({ labels, sourceId: "c", slot: null, into: "a" })).toEqual({
      parent: null,
      sortOrder: 5000,
    });
  });

  it("refuses self, own-child, and own-parent nest drops", () => {
    const labels = [...flat, row("kid", 12000, "a")];
    expect(resolveLabelDrop({ labels, sourceId: "a", slot: null, into: "a" })).toBe(null);
    expect(resolveLabelDrop({ labels, sourceId: "a", slot: null, into: "kid" })).toBe(null);
    expect(resolveLabelDrop({ labels, sourceId: "kid", slot: null, into: "a" })).toBe(null);
    expect(resolveLabelDrop({ labels, sourceId: "a", slot: null, into: "ghost" })).toBe(null);
  });

  it("moves rows across parents through sibling slots", () => {
    const labels = [...flat, row("kid", 12000, "a")];
    expect(resolveLabelDrop({ labels, sourceId: "kid", slot: { id: "b", after: false }, into: null })).toEqual({
      parent: null,
      sortOrder: 15000,
    });
    expect(resolveLabelDrop({ labels, sourceId: "b", slot: { id: "kid", after: false }, into: null })).toEqual({
      parent: "a",
      sortOrder: 6000,
    });
  });

  it("reorders a group above the parent when slotted among children", () => {
    const labels = [...flat, row("kid", 12000, "a"), row("gkid", 22000, "c")];
    expect(resolveLabelDrop({ labels, sourceId: "c", slot: { id: "kid", after: false }, into: null })).toEqual({
      parent: null,
      sortOrder: 5000,
    });
  });

  it("treats orphan rows as top level", () => {
    const labels = [...flat, row("orphan", 40000, "missing")];
    expect(resolveLabelDrop({ labels, sourceId: "a", slot: { id: "orphan", after: false }, into: null })).toEqual({
      parent: null,
      sortOrder: 35000,
    });
  });

  it("refuses a parent slotted at its own child", () => {
    const labels = [...flat, row("kid", 12000, "a")];
    expect(resolveLabelDrop({ labels, sourceId: "a", slot: { id: "kid", after: false }, into: null })).toBe(null);
    expect(resolveLabelDrop({ labels, sourceId: "a", slot: { id: "kid", after: true }, into: null })).toBe(null);
  });
});
