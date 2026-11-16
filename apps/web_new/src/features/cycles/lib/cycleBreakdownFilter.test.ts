// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Breakdown filter toggle (CYC-039/CYC-041): values toggle inside
// their dimension, emptied dimensions drop out, and foreign
// conditions survive untouched.
import { describe, expect, it } from "vitest";

import type { RichCondition } from "../../issues/index.js";
import { heroFilterKey, toggleBreakdownFilter } from "./cycleBreakdownFilter.js";

function condition(overrides: Partial<RichCondition> = {}): RichCondition {
  return { key: "seed-1", property: "state_group", operator: "in", negated: false, value: ["started"], ...overrides };
}

describe("heroFilterKey", () => {
  it("hands out fresh keys", () => {
    const first = heroFilterKey();
    const second = heroFilterKey();
    expect(first).toMatch(/^hero-filter-/);
    expect(second).not.toBe(first);
  });
});

describe("toggleBreakdownFilter", () => {
  it("appends a dimension onto empty filters", () => {
    const next = toggleBreakdownFilter([], "state_group", "backlog");
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({ property: "state_group", operator: "in", negated: false, value: ["backlog"] });
    expect(next[0]?.key).toMatch(/^hero-filter-/);
  });

  it("adds a second value to a matching dimension", () => {
    const next = toggleBreakdownFilter([condition()], "state_group", "backlog");
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({ key: "seed-1", value: ["started", "backlog"] });
  });

  it("removes a present value but keeps the dimension", () => {
    const seeded = condition({ value: ["started", "backlog"] });
    const next = toggleBreakdownFilter([seeded], "state_group", "backlog");
    expect(next).toHaveLength(1);
    expect(next[0]).toMatchObject({ key: "seed-1", value: ["started"] });
  });

  it("drops the dimension when its last value leaves", () => {
    const next = toggleBreakdownFilter([condition()], "state_group", "started");
    expect(next).toEqual([]);
  });

  it("keeps neighboring conditions while a matching dimension toggles", () => {
    const foreign = condition({ key: "other", property: "assignee_id", value: ["ada"] });
    const seeded = condition({ key: "seed-1", value: ["started", "backlog"] });
    const removed = toggleBreakdownFilter([foreign, seeded], "state_group", "backlog");
    expect(removed).toHaveLength(2);
    expect(removed[0]).toEqual(foreign);
    expect(removed[1]).toMatchObject({ key: "seed-1", value: ["started"] });
    const added = toggleBreakdownFilter([foreign, condition()], "state_group", "backlog");
    expect(added).toHaveLength(2);
    expect(added[0]).toEqual(foreign);
    expect(added[1]).toMatchObject({ key: "seed-1", value: ["started", "backlog"] });
  });

  it("toggles scalar and exact-operator dimensions under the in form", () => {
    const scalar = toggleBreakdownFilter([condition({ value: "started" })], "state_group", "backlog");
    expect(scalar[0]).toMatchObject({ operator: "in", value: ["started", "backlog"] });
    const exact = toggleBreakdownFilter([condition({ operator: "exact", value: "started" })], "state_group", "backlog");
    expect(exact[0]).toMatchObject({ operator: "in", value: ["started", "backlog"] });
    const nonString = toggleBreakdownFilter([condition({ value: 3 })], "state_group", "backlog");
    expect(nonString[0]).toMatchObject({ value: ["backlog"] });
  });

  it("leaves foreign, negated, and other-dimension conditions alone", () => {
    const foreign = condition({ key: "other", property: "assignee_id", value: ["ada"] });
    const negated = condition({ key: "neg", negated: true });
    const between = condition({ key: "range", operator: "between", value: ["a", "b"] });
    const next = toggleBreakdownFilter([foreign, negated, between], "state_group", "backlog");
    expect(next.slice(0, 3)).toEqual([foreign, negated, between]);
    expect(next[3]).toMatchObject({ property: "state_group", value: ["backlog"] });
  });
});
