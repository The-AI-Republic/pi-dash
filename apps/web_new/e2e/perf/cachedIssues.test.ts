// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The warm-start gate must recognise exactly the payload the measured
// relaunch can paint from. Each case below is a real shape the persistor
// can leave behind (NEWFRONT-886).
import { describe, expect, it } from "vitest";

import { cachedIssueListRows } from "./cachedIssues.js";

const SLUG = "perf-ws";
const PROJECT = "123e4567-e89b-42d3-a456-426614174001";
const OTHER_PROJECT = "123e4567-e89b-42d3-a456-426614174099";

function listKey(projectId: string = PROJECT, layout: string = "list"): unknown[] {
  return ["ws", SLUG, "issues", "list", projectId, { layout }, "sort_order"];
}

function payload(entries: unknown[]): string {
  return JSON.stringify({ version: 1, state: { queries: entries, mutations: [] } });
}

function listEntry(options: { projectId?: string; status?: string; rows?: unknown[]; data?: unknown } = {}) {
  const rows = options.rows ?? [{ id: "i1" }];
  return {
    queryKey: listKey(options.projectId),
    state: {
      status: options.status ?? "success",
      data: options.data ?? { results: rows, count: rows.length },
    },
  };
}

describe("cachedIssueListRows", () => {
  it("counts the rows of this project's list entry", () => {
    expect(cachedIssueListRows(payload([listEntry({ rows: [{ id: "a" }, { id: "b" }, { id: "c" }] })]), PROJECT)).toBe(
      3
    );
  });

  it("returns zero when the payload holds bootstrap queries but no issues entry", () => {
    const bootstrapOnly = payload([
      { queryKey: ["ws", SLUG, "reference", "states"], state: { status: "success", data: [] } },
      { queryKey: ["ws", SLUG, "me"], state: { status: "success", data: { id: "u1" } } },
    ]);
    expect(bootstrapOnly.length).toBeGreaterThan(0);
    expect(cachedIssueListRows(bootstrapOnly, PROJECT)).toBe(0);
  });

  it("returns zero when the baked list is empty or still pending", () => {
    expect(cachedIssueListRows(payload([listEntry({ rows: [] })]), PROJECT)).toBe(0);
    expect(cachedIssueListRows(payload([listEntry({ status: "pending" })]), PROJECT)).toBe(0);
  });

  it("accepts an errored entry that still carries fetched rows", () => {
    expect(cachedIssueListRows(payload([listEntry({ status: "error", rows: [{ id: "a" }] })]), PROJECT)).toBe(1);
  });

  it("ignores another project's list and non-list issue queries", () => {
    const otherProject = payload([listEntry({ projectId: OTHER_PROJECT, rows: [{ id: "a" }, { id: "b" }] })]);
    expect(cachedIssueListRows(otherProject, PROJECT)).toBe(0);
    const detailOnly = payload([
      {
        queryKey: ["ws", SLUG, "issues", "detail", PROJECT, "issue-1"],
        state: { status: "success", data: { id: "i1" } },
      },
    ]);
    expect(cachedIssueListRows(detailOnly, PROJECT)).toBe(0);
  });

  it("takes the fullest view when the list is cached under several shapes", () => {
    const stored = payload([
      listEntry({ rows: [{ id: "a" }] }),
      {
        queryKey: listKey(PROJECT, "compact"),
        state: { status: "success", data: { results: [{ id: "a" }, { id: "b" }] } },
      },
    ]);
    expect(cachedIssueListRows(stored, PROJECT)).toBe(2);
  });

  it("returns zero for missing, empty, malformed and off-shape payloads", () => {
    expect(cachedIssueListRows(null, PROJECT)).toBe(0);
    expect(cachedIssueListRows("", PROJECT)).toBe(0);
    expect(cachedIssueListRows("{not json", PROJECT)).toBe(0);
    expect(cachedIssueListRows("null", PROJECT)).toBe(0);
    expect(cachedIssueListRows(JSON.stringify({ version: 1 }), PROJECT)).toBe(0);
    expect(cachedIssueListRows(JSON.stringify({ state: { queries: "nope" } }), PROJECT)).toBe(0);
  });

  it("skips entries that do not carry a list payload", () => {
    const entries = [
      null,
      "string-entry",
      [],
      { queryKey: "ws", state: { status: "success", data: { results: [{}] } } },
      { queryKey: ["ws", SLUG, "issues", "list"], state: { status: "success", data: { results: [{}] } } },
      { queryKey: listKey(), state: { status: "success" } },
      { queryKey: listKey(), state: { status: "success", data: null } },
      { queryKey: listKey(), state: { status: "success", data: { results: null } } },
      { queryKey: listKey(), state: "settled" },
      listEntry({ rows: [{ id: "a" }, { id: "b" }] }),
    ];
    expect(cachedIssueListRows(payload(entries), PROJECT)).toBe(2);
  });

  it("does not mistake a scoped non-workspace key or a reference list for cached rows", () => {
    const entries = [
      {
        queryKey: ["scope", SLUG, "issues", "list", PROJECT, {}, "sort_order"],
        state: { status: "success", data: { results: [{}] } },
      },
      {
        queryKey: ["ws", SLUG, "reference", "path", PROJECT, {}, "sort_order"],
        state: { status: "success", data: { results: [{}] } },
      },
    ];
    expect(cachedIssueListRows(payload(entries), PROJECT)).toBe(0);
  });
});
