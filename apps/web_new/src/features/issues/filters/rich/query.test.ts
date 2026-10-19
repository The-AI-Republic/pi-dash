// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { richFiltersParam, splitDateConditions, withRichFilters } from "./query.js";

describe("richFiltersParam", () => {
  it("omits an empty expression", () => {
    expect(richFiltersParam({})).toBeNull();
  });

  it("serializes leaves, AND and negation", () => {
    expect(richFiltersParam({ priority__in: "urgent" })).toBe('{"priority__in":"urgent"}');
    expect(richFiltersParam({ and: [{ priority__in: "urgent" }, { not: { state_id__in: ["a"] } }] })).toBe(
      '{"and":[{"priority__in":"urgent"},{"not":{"state_id__in":["a"]}}]}'
    );
  });
});

describe("withRichFilters", () => {
  it("leaves the query untouched for an empty expression", () => {
    const query = { perPage: 50 };
    expect(withRichFilters(query, {})).toBe(query);
  });

  it("adds the filters parameter beside existing ones", () => {
    expect(withRichFilters({ perPage: 50, filters: { state: "todo" } }, { priority__in: "urgent" })).toEqual({
      perPage: 50,
      filters: { state: "todo", filters: '{"priority__in":"urgent"}' },
    });
  });
});

describe("date conditions travel as legacy list parameters", () => {
  it("moves a preset out of the JSON tree and into the bare date parameter", () => {
    expect(withRichFilters({ perPage: 50 }, { start_date__exact: "1_weeks;after;fromnow" })).toEqual({
      perPage: 50,
      filters: { start_date: "1_weeks;after;fromnow" },
    });
  });

  it("keeps the remaining conditions in JSON beside the date parameter", () => {
    const query = withRichFilters({}, { and: [{ priority__in: "urgent" }, { target_date__exact: "2026-01-01" }] });
    expect(query.filters).toEqual({ filters: '{"priority__in":"urgent"}', target_date: "2026-01-01;after" });
  });

  it("keeps a bare custom pair as lower and upper bounds", () => {
    expect(withRichFilters({}, { start_date__range: "2026-01-01,2026-01-31" }).filters).toEqual({
      start_date: "2026-01-01;after,2026-01-31;before",
    });
  });

  it("collapses a single remaining child and drops an emptied tree", () => {
    expect(
      splitDateConditions({ and: [{ priority__in: "urgent" }, { start_date__exact: "1_weeks;after;fromnow" }] })
    ).toEqual({
      expression: { priority__in: "urgent" },
      dates: { start_date: ["1_weeks;after;fromnow"] },
    });
    expect(splitDateConditions({ and: [{ start_date__exact: "1_weeks;after;fromnow" }] })).toEqual({
      expression: {},
      dates: { start_date: ["1_weeks;after;fromnow"] },
    });
    expect(
      splitDateConditions({
        and: [{ start_date__exact: "1_weeks;after;fromnow" }, { target_date__exact: "2026-01-01" }],
      })
    ).toEqual({
      expression: {},
      dates: { start_date: ["1_weeks;after;fromnow"], target_date: ["2026-01-01;after"] },
    });
  });

  it("keeps negated non-date leaves and deep date buckets", () => {
    expect(
      splitDateConditions({
        and: [{ not: { priority__in: "urgent" } }, { not: { start_date__exact: "1_weeks;after;fromnow" } }],
      })
    ).toEqual({ expression: { not: { priority__in: "urgent" } }, dates: { start_date: ["1_weeks;after;fromnow"] } });
    expect(
      splitDateConditions({
        and: [
          { start_date__exact: "1_weeks;after;fromnow" },
          { and: [{ start_date__exact: "2_weeks;after;fromnow" }] },
        ],
      }).dates
    ).toEqual({ start_date: ["1_weeks;after;fromnow", "2_weeks;after;fromnow"] });
  });

  it("leaves odd shapes and unknown date directions alone", () => {
    const odd = { and: [42 as unknown, "leaf", { priority__in: ["urgent"] }] } as never;
    expect(splitDateConditions(odd)).toEqual({ expression: odd, dates: {} });
    expect(splitDateConditions({ weird__exact: "x" } as never)).toEqual({
      expression: { weird__exact: "x" },
      dates: {},
    });
    // A duplicate entry is applied once.
    expect(splitDateConditions({ start_date__exact: "1_weeks;after;fromnow,1_weeks;after;fromnow" }).dates).toEqual({
      start_date: ["1_weeks;after;fromnow"],
    });
  });

  it("applies an empty and to the query unchanged", () => {
    const query = { perPage: 10 };
    expect(withRichFilters(query, {})).toBe(query);
    expect(withRichFilters(query, { and: [] })).toEqual({ perPage: 10 });
  });
});
