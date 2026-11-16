// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { State } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { neighborSequence, resolveStateMove, STATE_ORDER_STEP } from "./ordering.js";
import { state, stateList, testRowId } from "./testFixtures.js";

describe("neighborSequence", () => {
  const rows = [{ sequence: 10 }, { sequence: 20 }, { sequence: 40 }];

  it("halves the first value at the head", () => {
    expect(neighborSequence(rows, 0)).toBe(5);
  });

  it("takes the midpoint between two neighbours", () => {
    expect(neighborSequence(rows, 1)).toBe(15);
    expect(neighborSequence(rows, 2)).toBe(30);
  });

  it("steps past the last value at the tail", () => {
    expect(neighborSequence(rows, 3)).toBe(40 + STATE_ORDER_STEP);
  });

  it("starts from zero for an empty destination", () => {
    expect(neighborSequence([], 0)).toBe(0);
  });
});

describe("resolveStateMove", () => {
  // backlog: a(10) b(20) c(30); unstarted: x(5)
  const rows = [
    state({ name: "a", group: "backlog", sequence: 10 }),
    state({ name: "b", group: "backlog", sequence: 20 }),
    state({ name: "c", group: "backlog", sequence: 30 }),
    state({ name: "x", group: "unstarted", sequence: 5 }),
  ];

  it("reorders above the hovered row within the group", () => {
    expect(resolveStateMove(rows, testRowId("c"), testRowId("a"), "above")).toEqual({
      stateId: testRowId("c"),
      group: "backlog",
      sequence: 5,
    });
  });

  it("reorders below a row using the next neighbour's midpoint", () => {
    expect(resolveStateMove(rows, testRowId("a"), testRowId("b"), "below")).toEqual({
      stateId: testRowId("a"),
      group: "backlog",
      sequence: 25,
    });
  });

  it("appends past the tail when dropped below the last row", () => {
    expect(resolveStateMove(rows, testRowId("a"), testRowId("c"), "below")).toEqual({
      stateId: testRowId("a"),
      group: "backlog",
      sequence: 30 + STATE_ORDER_STEP,
    });
  });

  it("moves across groups, taking the target group and its neighbours", () => {
    expect(resolveStateMove(rows, testRowId("a"), testRowId("x"), "above")).toEqual({
      stateId: testRowId("a"),
      group: "unstarted",
      sequence: 2.5,
    });
    expect(resolveStateMove(rows, testRowId("a"), testRowId("x"), "below")).toEqual({
      stateId: testRowId("a"),
      group: "unstarted",
      sequence: 5 + STATE_ORDER_STEP,
    });
  });

  it("refuses a drop that leaves the row where it already sits", () => {
    expect(resolveStateMove(rows, testRowId("a"), testRowId("b"), "above")).toBeNull();
    expect(resolveStateMove(rows, testRowId("b"), testRowId("a"), "below")).toBeNull();
    expect(resolveStateMove(rows, testRowId("c"), testRowId("b"), "below")).toBeNull();
  });

  it("refuses unknown or identical source and target", () => {
    expect(resolveStateMove(rows, "missing", testRowId("a"), "below")).toBeNull();
    expect(resolveStateMove(rows, testRowId("a"), "missing", "below")).toBeNull();
    expect(resolveStateMove(rows, testRowId("a"), testRowId("a"), "above")).toBeNull();
  });

  it("refuses a target outside the settings groups", () => {
    const stray = { ...state({ name: "triage", group: "backlog" }), group: "triage" } as unknown as State;
    expect(resolveStateMove([...rows, stray], testRowId("a"), stray.id, "above")).toBeNull();
  });

  it("orders against the destination list without the moving row", () => {
    const list = stateList([
      { name: "one", group: "test", sequence: 100 },
      { name: "two", group: "test", sequence: 200 },
      { name: "three", group: "test", sequence: 300 },
    ]);
    const move = resolveStateMove(list, testRowId("one"), testRowId("three"), "above");
    expect(move).toEqual({ stateId: testRowId("one"), group: "test", sequence: 250 });
  });
});
