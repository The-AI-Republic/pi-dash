// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { withIssueFields } from "./optimisticRows.js";

const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "423e4567-e89b-12d3-a456-426614174003";
const PATCH = { estimate_point: "point-9" };

function row(id: string): Record<string, unknown> {
  return { id, name: `issue ${id.slice(0, 4)}`, estimate_point: null };
}

describe("withIssueFields", () => {
  it("patches the matching row in a bare list and skips the rest", () => {
    expect(withIssueFields([row(ISSUE_ID), row(OTHER_ID), "stray"], ISSUE_ID, PATCH)).toEqual([
      { ...row(ISSUE_ID), ...PATCH },
      row(OTHER_ID),
      "stray",
    ]);
  });

  it("patches a detail object only when the id matches", () => {
    expect(withIssueFields(row(ISSUE_ID), ISSUE_ID, PATCH)).toEqual({ ...row(ISSUE_ID), ...PATCH });
    expect(withIssueFields(row(OTHER_ID), ISSUE_ID, PATCH)).toEqual(row(OTHER_ID));
    expect(withIssueFields(42, ISSUE_ID, PATCH)).toBe(42);
    expect(withIssueFields(null, ISSUE_ID, PATCH)).toBe(null);
  });

  it("patches rows inside a flat list envelope", () => {
    expect(withIssueFields({ count: 2, results: [row(ISSUE_ID), row(OTHER_ID)] }, ISSUE_ID, PATCH)).toEqual({
      count: 2,
      results: [{ ...row(ISSUE_ID), ...PATCH }, row(OTHER_ID)],
    });
  });

  it("patches rows inside grouped buckets and keeps stray entries", () => {
    expect(
      withIssueFields(
        { count: 1, results: { todo: { results: [row(ISSUE_ID)], total_results: 1 }, stray: 7 } },
        ISSUE_ID,
        PATCH
      )
    ).toEqual({
      count: 1,
      results: {
        todo: { results: [{ ...row(ISSUE_ID), ...PATCH }], total_results: 1 },
        stray: 7,
      },
    });
  });

  it("passes unknown records through untouched", () => {
    expect(withIssueFields({ filters: {} }, ISSUE_ID, PATCH)).toEqual({ filters: {} });
  });
});
