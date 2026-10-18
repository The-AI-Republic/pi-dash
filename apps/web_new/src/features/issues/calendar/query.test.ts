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
    const key = calendarListKey(
      "acme",
      { scope: "project", projectId: "project-1" },
      {
        after: "a",
        before: "b",
        perPage: 4,
      }
    );
    expect(key).toContain("calendar");
    expect(key.slice(-3)).toEqual(["a", "b", 4]);
  });

  it("scopes cycle and module windows to their own entity reads", async () => {
    const getJson = vi.fn(async (_url: string, _init: unknown) => ({
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
    }));
    const client = {
      getJson,
      parse: vi.fn((_schema: unknown, body: unknown) => body),
    } as never;
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
    const entity = { scope: "project", projectId: "project-1" } as const;
    const list = calendarQueryOptions(client, "acme", "project-1", entity, {
      after: "2026-09-27",
      before: "2026-11-07",
      perPage: 4,
    });
    await expect(list.queryFn?.({} as never)).resolves.toEqual(response);
    const day = calendarDayQueryOptions(client, "acme", "project-1", entity, {
      day: "2026-10-05",
      perPage: 4,
      cursor: "4:1:0",
    });
    await expect(day.queryFn?.({} as never)).resolves.toEqual(response);
  });

  it("reads project state colours", async () => {
    const client = {
      getJson: vi.fn(async (_url: string) => []),
      parse: vi.fn((_schema: unknown, body: unknown) => body),
    } as never;
    const states = calendarStatesQueryOptions(client, "acme", "project-1");
    expect(states.queryKey).toContain("calendar-states");
    await expect(states.queryFn?.({} as never)).resolves.toEqual([]);
  });
});
