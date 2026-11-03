// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  calendarDayQueryOptions,
  calendarListKey,
  calendarQueryOptions,
  calendarStatesQueryOptions,
  dayTotalFromResponse,
  daysBetween,
  rowsFromResponse,
  targetDateFilter,
} from "./query.js";

const EXPRESSION = { and: [{ priority__in: "urgent" }] };
const PROJECT = { scope: "project", projectId: "project-1" } as const;

function groupedEmpty() {
  return {
    grouped_by: "target_date",
    sub_grouped_by: null,
    total_count: 0,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: 0,
    total_results: 0,
    results: {},
  };
}

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
    const key = calendarListKey("acme", PROJECT, { after: "a", before: "b", perPage: 4 });
    expect(key).toContain("calendar");
    expect(key.slice(-4, -1)).toEqual(["a", "b", 4]);
  });

  it("keys a saved expression apart from an unscoped calendar", () => {
    const base = calendarListKey("acme", PROJECT, { after: "a", before: "b", perPage: 4 });
    const scoped = calendarListKey("acme", PROJECT, { after: "a", before: "b", perPage: 4 }, EXPRESSION);
    expect(base.at(-1)).toBe("all");
    expect(scoped.at(-1)).toContain("priority__in");
    expect(scoped).not.toEqual(base);
  });

  it("sends the stored expression in the window read", async () => {
    const getJson = vi.fn(async (_url: string, _init: unknown) => groupedEmpty());
    const client = { getJson, parse: vi.fn((_schema: unknown, body: unknown) => body) } as never;
    const options = calendarQueryOptions(
      client,
      "acme",
      "project-1",
      PROJECT,
      { after: "2026-10-05", before: "2026-10-05", perPage: 4 },
      EXPRESSION
    );
    await options.queryFn?.({} as never);
    const init = getJson.mock.calls[0]?.[1] as { query: Record<string, unknown> } | undefined;
    expect(String(init?.query["filters"])).toContain("priority__in");
    expect(String(init?.query["target_date"])).toContain("2026-10-05");
  });

  it("omits the filters parameter when the view stores no expression", async () => {
    const getJson = vi.fn(async (_url: string, _init: unknown) => groupedEmpty());
    const client = { getJson, parse: vi.fn((_schema: unknown, body: unknown) => body) } as never;
    await calendarQueryOptions(client, "acme", "project-1", PROJECT, {
      after: "2026-10-05",
      before: "2026-10-05",
      perPage: 4,
    }).queryFn?.({} as never);
    const init = getJson.mock.calls[0]?.[1] as { query: Record<string, unknown> } | undefined;
    expect(String(init?.query["filters"])).not.toContain("priority__in");
  });

  it("sends the stored expression when one tile pages", async () => {
    const getJson = vi.fn(async (_url: string, _init: unknown) => groupedEmpty());
    const client = { getJson, parse: vi.fn((_schema: unknown, body: unknown) => body) } as never;
    const options = calendarDayQueryOptions(
      client,
      "acme",
      "project-1",
      PROJECT,
      { day: "2026-10-05", perPage: 30, cursor: "30:1:0" },
      EXPRESSION
    );
    await options.queryFn?.({} as never);
    const init = getJson.mock.calls[0]?.[1] as { query: Record<string, unknown> } | undefined;
    expect(String(init?.query["filters"])).toContain("priority__in");
    expect(options.queryKey).toContain("2026-10-05");
  });

  it("keys a day with and without a cursor apart", () => {
    const base = calendarDayQueryOptions(
      {} as never,
      "acme",
      "project-1",
      PROJECT,
      { day: "2026-10-05", perPage: 4 },
      EXPRESSION
    );
    const paged = calendarDayQueryOptions(
      {} as never,
      "acme",
      "project-1",
      PROJECT,
      { day: "2026-10-05", perPage: 4, cursor: "4:1:0" },
      EXPRESSION
    );
    expect(base.queryKey.slice(-2)).toEqual(["", expect.stringContaining("priority__in")]);
    expect(paged.queryKey.slice(-2)).toEqual(["4:1:0", expect.stringContaining("priority__in")]);
  });

  it("scopes cycle and module windows to their own entity reads", async () => {
    const getJson = vi.fn(async (_url: string, _init: unknown) => groupedEmpty());
    const client = { getJson, parse: vi.fn((_schema: unknown, body: unknown) => body) } as never;
    const query = { after: "2026-10-05", before: "2026-10-05", perPage: 4 };
    const cycle = calendarQueryOptions(
      client,
      "acme",
      "project-1",
      { scope: "cycle", projectId: "project-1", cycleId: "cycle-1" },
      query
    );
    expect(cycle.queryKey).toContain("cycle");
    await cycle.queryFn?.({} as never);
    expect(getJson).toHaveBeenLastCalledWith(
      "/api/workspaces/acme/projects/project-1/cycles/cycle-1/cycle-issues/",
      expect.anything()
    );
    const moduleWindow = calendarQueryOptions(
      client,
      "acme",
      "project-1",
      { scope: "module", projectId: "project-1", moduleId: "module-1" },
      query
    );
    await moduleWindow.queryFn?.({} as never);
    expect(getJson).toHaveBeenLastCalledWith(
      "/api/workspaces/acme/projects/project-1/modules/module-1/issues/",
      expect.anything()
    );
  });

  it("derives counts from the grouped envelope", async () => {
    const states = calendarStatesQueryOptions({} as never, "acme", "project-1");
    expect(states.queryKey).toContain("calendar-states");
    const response = {
      ...groupedEmpty(),
      total_results: 7,
      results: { "2026-10-05": { total_results: 3, results: [] } },
    } as never;
    expect(rowsFromResponse(response)).toEqual([]);
    expect(dayTotalFromResponse(response, "2026-10-05")).toBe(3);
    expect(dayTotalFromResponse(response, "2026-10-06")).toBe(7);
  });
});
