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
        {
          count: 1,
          results: {
            todo: { results: [row(ISSUE_ID)], total_results: 1 },
            empty: { total_results: 0 },
            stray: 7,
          },
        },
        ISSUE_ID,
        PATCH
      )
    ).toEqual({
      count: 1,
      results: {
        todo: { results: [{ ...row(ISSUE_ID), ...PATCH }], total_results: 1 },
        empty: { total_results: 0 },
        stray: 7,
      },
    });
  });

  it("passes unknown records through untouched", () => {
    expect(withIssueFields({ filters: {} }, ISSUE_ID, PATCH)).toEqual({ filters: {} });
  });

  it("moves a row out of its bucket when the grouped dimension changes", () => {
    const grouped = {
      grouped_by: "state_id",
      total_count: 2,
      results: {
        "state-a": { results: [row(ISSUE_ID)], total_results: 1 },
        "state-b": { results: [row(OTHER_ID)], total_results: 1 },
        empty: { results: [], total_results: 0 },
      },
    };
    const patch = { state_id: "state-b" };
    expect(withIssueFields(grouped, ISSUE_ID, patch)).toEqual({
      grouped_by: "state_id",
      total_count: 2,
      results: {
        "state-b": {
          results: [row(OTHER_ID), { ...row(ISSUE_ID), ...patch }],
          total_results: 2,
        },
        empty: { results: [], total_results: 0 },
      },
    });
  });

  it("drops a bucket that empties and recreates a target bucket", () => {
    const grouped = {
      grouped_by: "priority",
      results: {
        high: { results: [row(ISSUE_ID)], total_results: 1 },
      },
    };
    expect(withIssueFields(grouped, ISSUE_ID, { priority: "low" })).toEqual({
      grouped_by: "priority",
      results: {
        low: { results: [{ ...row(ISSUE_ID), priority: "low" }], total_results: 1 },
      },
    });
  });

  it("puts null and empty dimension values in the None bucket", () => {
    const grouped = {
      grouped_by: "cycle_id",
      results: {
        "cycle-a": { results: [row(ISSUE_ID)], total_results: 1 },
      },
    };
    const cleared = withIssueFields(grouped, ISSUE_ID, { cycle_id: null }) as {
      results: Record<string, { results: unknown[]; total_results: number }>;
    };
    expect(Object.keys(cleared.results)).toEqual(["None"]);
    expect(cleared.results.None?.results).toEqual([{ ...row(ISSUE_ID), cycle_id: null }]);
  });

  it("leaves multi-value and unknown dimensions in place", () => {
    const grouped = {
      grouped_by: "assignees__id",
      results: {
        user: { results: [row(ISSUE_ID)], total_results: 1 },
      },
    };
    const painted = withIssueFields(grouped, ISSUE_ID, { assignee_ids: ["u2"] }) as {
      results: Record<string, { results: unknown[] }>;
    };
    expect(painted.results.user?.results).toEqual([{ ...row(ISSUE_ID), assignee_ids: ["u2"] }]);
  });

  it("patches rows inside sub-grouped buckets", () => {
    const grouped = {
      grouped_by: "priority",
      sub_grouped_by: "state_id",
      results: {
        high: {
          total_results: 1,
          results: { "state-a": { results: [row(ISSUE_ID)], total_results: 1 } },
        },
      },
    };
    const painted = withIssueFields(grouped, ISSUE_ID, { assignee_ids: ["u2"] }) as {
      results: Record<string, { results: Record<string, { results: unknown[] }> }>;
    };
    expect(painted.results.high?.results["state-a"]?.results).toEqual([{ ...row(ISSUE_ID), assignee_ids: ["u2"] }]);
  });

  it("keeps stray rows in the source bucket while moving the matching one", () => {
    const grouped = {
      grouped_by: "priority",
      results: {
        high: { results: [row(ISSUE_ID), row(OTHER_ID), "stray"], total_results: 3 },
      },
    };
    const painted = withIssueFields(grouped, ISSUE_ID, { priority: "low" }) as {
      results: Record<string, { results: unknown[] }>;
    };
    expect(painted.results.high?.results).toEqual([row(OTHER_ID), "stray"]);
    expect(painted.results.low?.results).toEqual([{ ...row(ISSUE_ID), priority: "low" }]);
  });

  it("keeps only fields the row already carries when painting a list row", () => {
    expect(
      withIssueFields(
        [row(ISSUE_ID)],
        ISSUE_ID,
        { ...PATCH, description_html: "<p>x</p>" },
        {
          knownFieldsOnly: true,
        }
      )
    ).toEqual([{ ...row(ISSUE_ID), ...PATCH }]);
  });

  it("leaves a bucket alone when the row is not in any of them", () => {
    const grouped = {
      grouped_by: "priority",
      results: { high: { results: [row(OTHER_ID)], total_results: 1 } },
    };
    expect(withIssueFields(grouped, ISSUE_ID, { priority: "low" })).toEqual(grouped);
  });

  it("skips buckets without a row list and counts missing totals as one", () => {
    const grouped = {
      grouped_by: "priority",
      results: {
        broken: { results: "not a list" },
        stray: 7,
        high: { results: [row(ISSUE_ID)] },
        low: { results: [row(OTHER_ID)] },
      },
    };
    expect(withIssueFields(grouped, ISSUE_ID, { priority: "low" })).toEqual({
      grouped_by: "priority",
      results: {
        broken: { results: "not a list" },
        stray: 7,
        low: {
          results: [row(OTHER_ID), { ...row(ISSUE_ID), priority: "low" }],
          total_results: 1,
        },
      },
    });
  });

  it("does not bucket a row on a non-primitive grouping value", () => {
    const grouped = {
      grouped_by: "priority",
      results: { high: { results: [row(ISSUE_ID)], total_results: 1 } },
    };
    const painted = withIssueFields(grouped, ISSUE_ID, { priority: { nested: true } }) as {
      results: Record<string, { results: unknown[] }>;
    };
    expect(painted.results.high?.results).toEqual([{ ...row(ISSUE_ID), priority: { nested: true } }]);
  });

  it("keeps the source bucket count honest when the totals are missing", () => {
    const grouped = {
      grouped_by: "priority",
      results: {
        high: { results: [row(ISSUE_ID), row(OTHER_ID)] },
      },
    };
    expect(withIssueFields(grouped, ISSUE_ID, { priority: "low" })).toEqual({
      grouped_by: "priority",
      results: {
        high: { results: [row(OTHER_ID)], total_results: 0 },
        low: { results: [{ ...row(ISSUE_ID), priority: "low" }], total_results: 1 },
      },
    });
  });
});
