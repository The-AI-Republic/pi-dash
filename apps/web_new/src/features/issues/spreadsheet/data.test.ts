// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import type { IssueListItem, IssueListResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  flattenSheetPages,
  insertSheetTempRow,
  removeSheetTempRow,
  replaceTempWithCreated,
  sheetFailureDetail,
  sheetTempIssueRow,
  sheetTotalCount,
  withSheetIssueFields,
  withSheetPages,
} from "./data.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ID1 = "123e4567-e89b-12d3-a456-426614174000";
const ID2 = "323e4567-e89b-12d3-a456-426614174002";

function row(id: string, name: string, overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id,
    name,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 1,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function page(results: IssueListItem[], overrides: Partial<IssueListResponse> = {}): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
    ...overrides,
  };
}

function infinite(pages: IssueListResponse[]): { pages: IssueListResponse[]; pageParams: unknown[] } {
  return { pages, pageParams: pages.map(() => "") };
}

describe("spreadsheet data", () => {
  it("flattens loaded pages in order and reads the server total", () => {
    const data = infinite([
      page([row(ID1, "First")], { total_count: 3 }),
      page([row(ID2, "Second")], { total_count: 3 }),
    ]);
    expect(flattenSheetPages(data).map((issue) => issue.name)).toEqual(["First", "Second"]);
    expect(sheetTotalCount(data)).toBe(3);
    expect(flattenSheetPages(undefined)).toEqual([]);
    expect(sheetTotalCount(undefined)).toBe(0);
  });

  it("leaves values that are not infinite pages untouched", () => {
    const flat = [row(ID1, "First")];
    expect(withSheetPages(flat, (entry) => entry)).toBe(flat);
    expect(withSheetPages({ pages: "nope" }, (entry) => entry)).toEqual({ pages: "nope" });
    expect(withSheetPages(null, (entry) => entry)).toBeNull();
    expect(withSheetPages({ pages: [{ results: "bad" }], pageParams: [] }, (entry) => entry)).toEqual({
      pages: [{ results: "bad" }],
      pageParams: [],
    });
  });

  it("inserts a pending row at the head of the first page exactly once", () => {
    const temp = row("temp-1", "Draft");
    const data = infinite([page([row(ID1, "First")]), page([row(ID2, "Second")])]);
    const once = insertSheetTempRow(data, temp) as ReturnType<typeof infinite>;
    expect(once.pages[0]?.results.map((issue) => issue.id)).toEqual(["temp-1", ID1]);
    expect(once.pages[0]?.count).toBe(2);
    expect(once.pages[0]?.total_count).toBe(2);
    expect(once.pages[0]?.total_results).toBe(2);
    expect(once.pages[1]?.results.map((issue) => issue.id)).toEqual([ID2]);
    expect(once.pages[1]?.total_count).toBe(1);
    const twice = insertSheetTempRow(once, temp) as ReturnType<typeof infinite>;
    expect(twice.pages[0]?.results.map((issue) => issue.id)).toEqual(["temp-1", ID1]);
  });

  it("removes a pending row and clamps counts at zero", () => {
    const data = infinite([
      page([row("temp-1", "Draft"), row(ID1, "First")], { count: 0, total_count: 0, total_results: 0 }),
    ]);
    const cleaned = removeSheetTempRow(data, "temp-1") as ReturnType<typeof infinite>;
    expect(cleaned.pages[0]?.results.map((issue) => issue.id)).toEqual([ID1]);
    expect(cleaned.pages[0]?.count).toBe(0);
    expect(cleaned.pages[0]?.total_count).toBe(0);
    expect(removeSheetTempRow(cleaned, "temp-1")).toBe(cleaned);
  });

  it("patches a row's fields on every page only when the id matches", () => {
    const data = infinite([page([row(ID1, "First")]), page([row(ID2, "Second"), row(ID1, "Again")])]);
    const next = withSheetIssueFields(data, ID1, { priority: "high", name: "Renamed" }) as ReturnType<typeof infinite>;
    expect(next.pages[0]?.results[0]).toMatchObject({ priority: "high", name: "Renamed" });
    expect(next.pages[1]?.results[0]?.name).toBe("Second");
    expect(next.pages[1]?.results[1]?.name).toBe("Renamed");
  });

  it("builds a synthetic row for the pending quick-add", () => {
    const temp = sheetTempIssueRow("temp-abc", PROJECT, "New work");
    expect(temp).toMatchObject({
      id: "temp-abc",
      name: "New work",
      project_id: PROJECT,
      sequence_id: 0,
      priority: "none",
      is_draft: false,
    });
  });

  it("swaps the oldest temp row for the created server row", () => {
    const created = { ...row(ID1, "Confirmed"), deleted_at: null };
    const data = infinite([page([row("temp-1", "Draft"), row(ID2, "Second")])]);
    const next = replaceTempWithCreated(data, created) as ReturnType<typeof infinite>;
    expect(next.pages[0]?.results[0]).toMatchObject({ id: ID1, name: "Confirmed" });
    expect(next.pages[0]?.results[0]).not.toHaveProperty("deleted_at");
    expect(next.pages[0]?.results[1]?.id).toBe(ID2);
    expect(replaceTempWithCreated(data, { ...created, deleted_at: null })).toBeDefined();
  });

  it("leaves pages without a temp row unchanged on replace", () => {
    const data = infinite([page([row(ID1, "First")])]);
    const next = replaceTempWithCreated(data, { ...row(ID2, "Second"), deleted_at: null }) as ReturnType<
      typeof infinite
    >;
    expect(next.pages[0]).toBe(data.pages[0]);
  });

  it("uses the API message for refusals and the shared copy otherwise", () => {
    expect(sheetFailureDetail(new Error("boom"))).toBe("The cell is unchanged — check your connection and try again.");
    expect(sheetFailureDetail({ message: "nope" })).toBe(
      "The cell is unchanged — check your connection and try again."
    );
    expect(sheetFailureDetail(new ApiError({ status: 400, code: "http", message: "State is required" }))).toBe(
      "State is required"
    );
    expect(sheetFailureDetail(new ApiError({ status: 400, code: "http", message: "" }))).toBe(
      "The cell is unchanged — check your connection and try again."
    );
  });
});
