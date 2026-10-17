// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isCompletedCycle, selectCycleOptions } from "./cycles.js";

const CURRENT = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DONE = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const PROJECT = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

function cycleRow(id: string, name: string, status?: string) {
  return { id, project_id: PROJECT, name, is_favorite: false, ...(status === undefined ? {} : { status }) };
}

describe("isCompletedCycle", () => {
  it("matches completed statuses case-insensitively", () => {
    expect(isCompletedCycle("COMPLETED")).toBe(true);
    expect(isCompletedCycle("completed")).toBe(true);
    expect(isCompletedCycle("ACTIVE")).toBe(false);
    expect(isCompletedCycle(undefined)).toBe(false);
  });
});

describe("selectCycleOptions", () => {
  it("leads with the clear row and drops completed cycles", () => {
    const options = selectCycleOptions({
      cycles: [cycleRow(CURRENT, "Current", "ACTIVE"), cycleRow(DONE, "Done", "COMPLETED")],
    });
    expect(options).toEqual([
      { id: "", name: "No cycle", clear: true },
      { id: CURRENT, name: "Current", clear: false },
    ]);
  });

  it("keeps rows with a missing status and excludes on request", () => {
    const options = selectCycleOptions({
      cycles: [cycleRow(CURRENT, "Current")],
      excludeIds: [CURRENT],
      allowClear: false,
    });
    expect(options).toEqual([]);
  });

  it("keeps the assigned cycle listed by default", () => {
    const options = selectCycleOptions({ cycles: [cycleRow(CURRENT, "Current", "ACTIVE")] });
    expect(options.map((option) => option.id)).toEqual(["", CURRENT]);
  });
});
