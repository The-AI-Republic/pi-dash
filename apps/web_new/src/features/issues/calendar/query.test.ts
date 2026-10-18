// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  calendarDayQueryOptions,
  calendarListKey,
  calendarQueryOptions,
  calendarSearchQueryOptions,
  calendarStatesQueryOptions,
  dayTotalFromResponse,
  daysBetween,
  rowsFromResponse,
  targetDateFilter,
} from "./query.js";

describe("calendar queries", () => {
  it("lists inclusive ISO days", () => {
    expect(daysBetween("2026-10-30", "2026-11-02")).toEqual(["2026-10-30", "2026-10-31", "2026-11-01", "2026-11-02"]);
    expect(daysBetween("2026-10-30", "2026-10-30")).toEqual(["2026-10-30"]);
    expect(daysBetween("2026-11-02", "2026-10-30")).toEqual([]);
    expect(daysBetween("nope", "2026-10-30")).toEqual([]);
  });

  it("encodes the target-date window filter", () => {
    expect(targetDateFilter({ after: "2026-09-27", before: "2026-11-07" })).toBe("2026-09-27;after,2026-11-07;before");
  });

  it("builds range-scoped list keys", () => {
    const key = calendarListKey("acme", "project-1", { after: "a", before: "b", perPage: 4 });
    expect(key).toContain("calendar");
    expect(key.slice(-3)).toEqual(["a", "b", 4]);
  });

  it("flattens every grouped bucket", () => {
    expect(
      rowsFromResponse({
        grouped_by: "target_date",
        sub_grouped_by: null,
        total_count: 2,
        next_cursor: "4:1:0",
        prev_cursor: "4:-1:0",
        next_page_results: false,
        prev_page_results: false,
        count: 1,
        total_results: 2,
        results: {
          "2026-10-05": { results: [{ id: "a" } as never], total_results: 1 },
          "2026-10-06": { results: [{ id: "b" } as never], total_results: 1 },
        },
      }).map((row) => row.id)
    ).toEqual(["a", "b"]);
  });

  it("reads a day bucket's total rather than the envelope total", () => {
    const response = {
      grouped_by: "target_date",
      sub_grouped_by: null,
      total_count: 0,
      next_cursor: "4:1:0",
      prev_cursor: "",
      next_page_results: true,
      prev_page_results: false,
      count: 1,
      total_results: 0,
      results: { "2026-10-05": { results: [{ id: "a" } as never], total_results: 7 } },
    };
    expect(dayTotalFromResponse(response, "2026-10-05")).toBe(7);
    expect(dayTotalFromResponse(response, "2026-10-06")).toBe(0);
  });

  it("parses the grouped response for the list and day reads", async () => {
    const response = {
      grouped_by: "target_date",
      sub_grouped_by: null,
      total_count: 0,
      next_cursor: "4:1:0",
      prev_cursor: "",
      next_page_results: false,
      prev_page_results: false,
      count: 0,
      total_results: 0,
      results: {},
    };
    const client = {
      getJson: vi.fn(async () => response),
      parse: vi.fn((_schema: unknown, body: unknown) => body),
    } as never;
    const list = calendarQueryOptions(client, "acme", "project-1", {
      after: "2026-09-27",
      before: "2026-11-07",
      perPage: 4,
    });
    await expect(list.queryFn?.({} as never)).resolves.toEqual(response);
    const day = calendarDayQueryOptions(client, "acme", "project-1", {
      day: "2026-10-05",
      perPage: 4,
      cursor: "4:1:0",
    });
    await expect(day.queryFn?.({} as never)).resolves.toEqual(response);
  });

  it("reads add-existing choices and project state colours", async () => {
    const client = {
      getJson: vi.fn(async (url: string) => (url.includes("search-issues") ? [] : [])),
      parse: vi.fn((_schema: unknown, body: unknown) => body),
    } as never;
    const search = calendarSearchQueryOptions(client, "acme", "project-1");
    expect(search.queryKey).toContain("calendar-search");
    await expect(search.queryFn?.({} as never)).resolves.toEqual([]);
    const states = calendarStatesQueryOptions(client, "acme", "project-1");
    expect(states.queryKey).toContain("calendar-states");
    await expect(states.queryFn?.({} as never)).resolves.toEqual([]);
  });
});
