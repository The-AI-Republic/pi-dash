// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board drop resolution (parity/specs/issues.md §5, ISS-037–040): the
// delete zone wins, then the card under the cursor with its insertion
// edge, then the bare cell or column beneath — by hit-test first and by
// rect scan when the document cannot hit-test.

import { describe, expect, it } from "vitest";

import {
  boardDropAtPoint,
  KANBAN_CARD_SELECTOR,
  KANBAN_CELL_SELECTOR,
  KANBAN_COLUMN_SELECTOR,
  KANBAN_DELETE_ZONE_SELECTOR,
} from "./dropTarget.js";

interface FakeRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

interface FakeElement {
  closest: (selectors: string) => FakeElement | null;
  getAttribute: (name: string) => string | null;
  getBoundingClientRect: () => FakeRect;
}

function rect(top: number, bottom: number, left = 0, right = 100): FakeRect {
  return { left, top, right, bottom };
}

function node(
  attrs: Record<string, string | null>,
  bounds: FakeRect,
  ancestors: Record<string, FakeElement | null> = {}
): FakeElement {
  return {
    closest: (selectors: string) => ancestors[selectors] ?? null,
    getAttribute: (name: string) => attrs[name] ?? null,
    getBoundingClientRect: () => bounds,
  };
}

function docOf(
  elements: Record<string, FakeElement[]>,
  hit: FakeElement | null = null
): {
  elementFromPoint: () => FakeElement | null;
  querySelectorAll: (selectors: string) => FakeElement[];
} {
  return {
    elementFromPoint: () => hit,
    querySelectorAll: (selectors: string) => elements[selectors] ?? [],
  };
}

const COLUMN = "323e4567-e89b-12d3-a456-426614174002";
const LANE = "label-1";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function cardNode(top = 100, bottom = 180): FakeElement {
  return node({ "data-issue-id": ISSUE, "data-group-key": COLUMN, "data-lane-key": LANE }, rect(top, bottom));
}

describe("boardDropAtPoint", () => {
  it("resolves nothing without a document or with wild coordinates", () => {
    expect(boardDropAtPoint(null, 10, 10)).toBeNull();
    expect(boardDropAtPoint(undefined, 10, 10)).toBeNull();
    expect(boardDropAtPoint(docOf({}), Number.NaN, 10)).toBeNull();
    expect(boardDropAtPoint(docOf({}), 10, Number.POSITIVE_INFINITY)).toBeNull();
  });

  it("scans when the hit-test misses or cannot climb", () => {
    expect(boardDropAtPoint(docOf({}), 50, 120)).toBeNull();
    const bare = { getAttribute: () => null, getBoundingClientRect: () => rect(100, 180) };
    const scan = {
      elementFromPoint: () => bare,
      querySelectorAll: () => [],
    };
    expect(boardDropAtPoint(scan, 50, 120)).toBeNull();
  });

  it("prefers the delete zone over everything beneath it", () => {
    const zone = node({}, rect(0, 40));
    const hit = node({}, rect(0, 40), { [KANBAN_DELETE_ZONE_SELECTOR]: zone });
    expect(boardDropAtPoint(docOf({}, hit), 50, 20)).toEqual({
      kind: "delete",
      columnKey: null,
      laneKey: null,
      issueId: null,
      edge: null,
    });
  });

  it("lets the zone win when the hit falls through to a cell beneath it", () => {
    const zone = node({}, rect(0, 40));
    const cell = node({ "data-column-key": COLUMN, "data-lane-key": LANE }, rect(0, 400));
    // The zone is pointer-events-none, so elementFromPoint returns the
    // board element beneath it; the hit climbs to a cell, not the zone.
    const hit = node({}, rect(0, 400), { [KANBAN_CELL_SELECTOR]: cell });
    const elements = { [KANBAN_DELETE_ZONE_SELECTOR]: [zone] };
    expect(boardDropAtPoint(docOf(elements, hit), 50, 20)).toEqual({
      kind: "delete",
      columnKey: null,
      laneKey: null,
      issueId: null,
      edge: null,
    });
  });

  it("reads the card and its insertion edge from the hit", () => {
    const card = cardNode();
    const above = node({}, rect(100, 180), { [KANBAN_CARD_SELECTOR]: card });
    expect(boardDropAtPoint(docOf({}, above), 50, 120)).toEqual({
      kind: "card",
      columnKey: COLUMN,
      laneKey: LANE,
      issueId: ISSUE,
      edge: "above",
    });
    const below = node({}, rect(100, 180), { [KANBAN_CARD_SELECTOR]: card });
    expect(boardDropAtPoint(docOf({}, below), 50, 160)).toEqual({
      kind: "card",
      columnKey: COLUMN,
      laneKey: LANE,
      issueId: ISSUE,
      edge: "below",
    });
  });

  it("reads bare cells and columns from the hit", () => {
    const cell = node({ "data-column-key": COLUMN, "data-lane-key": LANE }, rect(0, 400));
    const cellHit = node({}, rect(0, 400), { [KANBAN_CELL_SELECTOR]: cell });
    expect(boardDropAtPoint(docOf({}, cellHit), 50, 300)).toEqual({
      kind: "cell",
      columnKey: COLUMN,
      laneKey: LANE,
      issueId: null,
      edge: null,
    });
    const column = node({ "data-column-key": COLUMN }, rect(0, 600));
    const columnHit = node({}, rect(0, 600), { [KANBAN_COLUMN_SELECTOR]: column });
    expect(boardDropAtPoint(docOf({}, columnHit), 50, 500)).toEqual({
      kind: "column",
      columnKey: COLUMN,
      laneKey: null,
      issueId: null,
      edge: null,
    });
  });

  it("treats an empty lane marker as flat", () => {
    const card = node({ "data-issue-id": ISSUE, "data-group-key": COLUMN, "data-lane-key": "" }, rect(100, 180));
    const hit = node({}, rect(100, 180), { [KANBAN_CARD_SELECTOR]: card });
    expect(boardDropAtPoint(docOf({}, hit), 50, 120)?.laneKey).toBeNull();
  });

  it("scans rects when the hit climbs nowhere", () => {
    const elements = {
      [KANBAN_DELETE_ZONE_SELECTOR]: [],
      [KANBAN_CARD_SELECTOR]: [cardNode()],
      [KANBAN_CELL_SELECTOR]: [],
      [KANBAN_COLUMN_SELECTOR]: [],
    };
    const hit = node({}, rect(100, 180));
    expect(boardDropAtPoint(docOf(elements, hit), 50, 120)?.kind).toBe("card");
  });

  it("scans every layer without a hit-tester", () => {
    const elements: Record<string, FakeElement[]> = {
      [KANBAN_DELETE_ZONE_SELECTOR]: [node({}, rect(0, 40))],
      [KANBAN_CARD_SELECTOR]: [cardNode()],
      [KANBAN_CELL_SELECTOR]: [node({ "data-column-key": COLUMN, "data-lane-key": LANE }, rect(0, 400))],
      [KANBAN_COLUMN_SELECTOR]: [node({ "data-column-key": COLUMN }, rect(0, 600))],
    };
    const scan = { querySelectorAll: (selectors: string) => elements[selectors] ?? [] };
    expect(boardDropAtPoint(scan, 50, 20)?.kind).toBe("delete");
    expect(boardDropAtPoint(scan, 50, 120)?.kind).toBe("card");
    expect(boardDropAtPoint(scan, 50, 300)?.kind).toBe("cell");
    expect(boardDropAtPoint(scan, 50, 500)?.kind).toBe("column");
    expect(boardDropAtPoint(scan, 50, 700)).toBeNull();
  });

  it("skips flat rects and markers without keys", () => {
    const elements: Record<string, FakeElement[]> = {
      [KANBAN_DELETE_ZONE_SELECTOR]: [node({}, rect(50, 50))],
      [KANBAN_CARD_SELECTOR]: [node({ "data-issue-id": null, "data-group-key": null }, rect(100, 180))],
      [KANBAN_CELL_SELECTOR]: [node({ "data-column-key": null }, rect(0, 400))],
      [KANBAN_COLUMN_SELECTOR]: [node({ "data-column-key": null }, rect(0, 600))],
    };
    const scan = { querySelectorAll: (selectors: string) => elements[selectors] ?? [] };
    expect(boardDropAtPoint(scan, 50, 50)).toBeNull();
    expect(boardDropAtPoint(scan, 50, 120)).toBeNull();
    expect(boardDropAtPoint(scan, 50, 300)).toBeNull();
    expect(boardDropAtPoint(scan, 50, 500)).toBeNull();
  });

  it("skips a zero-width rect even when its marker has keys", () => {
    const elements: Record<string, FakeElement[]> = {
      [KANBAN_DELETE_ZONE_SELECTOR]: [],
      [KANBAN_CARD_SELECTOR]: [],
      [KANBAN_CELL_SELECTOR]: [],
      [KANBAN_COLUMN_SELECTOR]: [node({ "data-column-key": COLUMN }, { left: 50, top: 0, right: 50, bottom: 600 })],
    };
    const scan = { querySelectorAll: (selectors: string) => elements[selectors] ?? [] };
    expect(boardDropAtPoint(scan, 50, 500)).toBeNull();
  });
});
