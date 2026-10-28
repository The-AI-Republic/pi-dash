// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue reads (parity/specs/issues.md §15 + §22 ISS-238): one cursor
// page per request at the fifty-row cap, the next page only when the
// envelope carries a cursor, the loaded rows in page order, and the
// add-existing search's project-scoped parameters.
import { ApiError, type ApiClient, type SubIssuesResponse } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import * as api from "@pidash/api-client";
import { issueKeys } from "../api/keys.js";
import {
  failureDetail,
  latestEnvelope,
  loadedRows,
  subIssueSearchKey,
  subIssueSearchQueryOptions,
  listChildren,
  subIssuesKey,
  subIssuesQueryOptions,
} from "./queries.js";
import { SUB_ISSUE_PAGE_SIZE } from "./rules.js";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, listSubIssues: vi.fn(), listEpicWorkItems: vi.fn(), searchIssues: vi.fn() };
});

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";

const CLIENT = { name: "stub-client" } as unknown as ApiClient;

function envelope(overrides: Partial<SubIssuesResponse> = {}): SubIssuesResponse {
  return { sub_issues: [], state_distribution: {}, ...overrides };
}

describe("sub-issue query", () => {
  it("scopes the key under the workspace and the parent", () => {
    expect(subIssuesKey(WS, PROJECT, PARENT)).toEqual([...issueKeys.all(WS), "sub-issues", PROJECT, PARENT]);
  });

  it("starts at no cursor and asks for fifty rows", async () => {
    vi.mocked(api.listSubIssues).mockResolvedValue(envelope());
    const options = subIssuesQueryOptions(CLIENT, WS, PROJECT, PARENT);
    await options.queryFn({ pageParam: options.initialPageParam });
    expect(api.listSubIssues).toHaveBeenCalledWith(CLIENT, WS, PROJECT, PARENT, { perPage: SUB_ISSUE_PAGE_SIZE });
  });

  it("carries the cursor of a later page", async () => {
    vi.mocked(api.listSubIssues).mockResolvedValue(envelope());
    const options = subIssuesQueryOptions(CLIENT, WS, PROJECT, PARENT);
    await options.queryFn({ pageParam: { cursor: "c2" } });
    expect(api.listSubIssues).toHaveBeenLastCalledWith(CLIENT, WS, PROJECT, PARENT, {
      perPage: SUB_ISSUE_PAGE_SIZE,
      cursor: "c2",
    });
  });

  it("reads an epic parent through the epics children collection", async () => {
    vi.mocked(api.listEpicWorkItems).mockResolvedValue(envelope());
    vi.mocked(api.listSubIssues).mockClear();
    const options = subIssuesQueryOptions(CLIENT, WS, PROJECT, PARENT, true);
    await options.queryFn({ pageParam: options.initialPageParam });
    expect(api.listEpicWorkItems).toHaveBeenCalledWith(CLIENT, WS, PROJECT, PARENT, { perPage: SUB_ISSUE_PAGE_SIZE });
    expect(api.listSubIssues).not.toHaveBeenCalled();
  });

  it("keeps one cache key per parent regardless of kind", () => {
    expect(subIssuesQueryOptions(CLIENT, WS, PROJECT, PARENT, true).queryKey).toEqual(
      subIssuesQueryOptions(CLIENT, WS, PROJECT, PARENT, false).queryKey
    );
  });

  it("passes the cursor through to the epic read", async () => {
    vi.mocked(api.listEpicWorkItems).mockResolvedValue(envelope());
    await listChildren(
      CLIENT,
      { workspaceSlug: WS, projectId: PROJECT, parentId: PARENT, isEpic: true },
      { perPage: 50, cursor: "c9" }
    );
    expect(api.listEpicWorkItems).toHaveBeenCalledWith(CLIENT, WS, PROJECT, PARENT, { perPage: 50, cursor: "c9" });
  });

  it("asks for the next page only while the envelope carries a cursor", () => {
    const options = subIssuesQueryOptions(CLIENT, WS, PROJECT, PARENT);
    expect(options.getNextPageParam(envelope({ next_cursor: "c2" }))).toEqual({ cursor: "c2" });
    expect(options.getNextPageParam(envelope({ next_cursor: "" }))).toBeUndefined();
    expect(options.getNextPageParam(envelope())).toBeUndefined();
  });

  it("flattens the loaded pages and reads the first envelope", () => {
    const first = envelope({ total_count: 9 });
    const second = envelope({ total_count: 9 });
    expect(loadedRows(undefined)).toEqual([]);
    expect(loadedRows({ pages: [first, second] })).toEqual([]);
    expect(latestEnvelope({ pages: [first, second] })).toBe(first);
    expect(latestEnvelope(undefined)).toBeUndefined();
  });

  it("keeps every row across pages in page order", () => {
    const one = envelope({ sub_issues: [{ id: "a" } as never] });
    const two = envelope({ sub_issues: [{ id: "b" } as never] });
    expect(loadedRows({ pages: [one, two] }).map((row) => row.id)).toEqual(["a", "b"]);
  });
});

describe("add-existing search", () => {
  it("scopes the search under the project and the term", () => {
    expect(subIssueSearchKey(WS, PROJECT, "alpha")).toEqual([
      ...issueKeys.all(WS),
      "sub-issues",
      "search",
      PROJECT,
      "alpha",
    ]);
  });

  it("searches the project with the sub-issue exclusion", async () => {
    vi.mocked(api.searchIssues).mockResolvedValue([]);
    const options = subIssueSearchQueryOptions(CLIENT, WS, PROJECT, "alpha");
    const run = options.queryFn;
    if (run === undefined) throw new Error("search options carry a query function");
    await run({} as never);
    expect(api.searchIssues).toHaveBeenCalledWith(CLIENT, WS, PROJECT, { search: "alpha", subIssue: true });
  });

  it("stays disabled without a project", () => {
    expect(subIssueSearchQueryOptions(CLIENT, WS, "", "").enabled).toBe(false);
    expect(subIssueSearchQueryOptions(CLIENT, WS, PROJECT, "").enabled).toBe(true);
  });
});

describe("failureDetail", () => {
  it("uses the server's message verbatim", () => {
    expect(failureDetail(new ApiError({ status: 500, code: "server", message: "server said no" }), "fallback")).toBe(
      "server said no"
    );
    expect(
      failureDetail(new ApiError({ status: 400, code: "validation", message: " Title is required " }), "fallback")
    ).toBe(" Title is required ");
  });

  it("falls back when the message is blank", () => {
    expect(failureDetail(new ApiError({ status: 500, code: "server", message: "" }), "fallback")).toBe("fallback");
    expect(failureDetail(new ApiError({ status: 500, code: "server", message: "   " }), "fallback")).toBe("fallback");
    expect(failureDetail(new ApiError({ status: 500, code: "server", message: "\n\t " }), "fallback")).toBe("fallback");
  });

  it("falls back when the message is an empty JSON shell", () => {
    for (const message of ["{}", "[]", "null", " {} ", "[]\n", "null "]) {
      expect(failureDetail(new ApiError({ status: 500, code: "http", message }), "fallback")).toBe("fallback");
    }
  });

  it("keeps a JSON body that carries content", () => {
    const cases: Array<[string, string]> = [
      ['{"detail":"nope"}', '{"detail":"nope"}'],
      ["[1]", "[1]"],
      ["true", "true"],
      ["0", "0"],
      ['"plain string"', '"plain string"'],
      ["{", "{"],
    ];
    for (const [message, expected] of cases) {
      expect(failureDetail(new ApiError({ status: 500, code: "http", message }), "fallback")).toBe(expected);
    }
  });

  it("ignores errors that are not API errors, and non-string messages", () => {
    expect(failureDetail(new Error("{}"), "fallback")).toBe("fallback");
    expect(failureDetail({ nope: true }, "fallback")).toBe("fallback");
    expect(failureDetail({ message: "looks like one" }, "fallback")).toBe("fallback");
    expect(failureDetail(undefined, "fallback")).toBe("fallback");
    const nonString = new ApiError({ status: 500, code: "server", message: "saved" });
    Object.defineProperty(nonString, "message", { value: 42, configurable: true });
    expect(failureDetail(nonString, "fallback")).toBe("fallback");
  });
});
