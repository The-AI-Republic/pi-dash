// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  getIssue: vi.fn(),
  searchWorkspace: vi.fn(),
}));

import { getIssue, searchWorkspace, type Issue } from "@pidash/api-client";

import { browseIssueQueryOptions, loadBrowseIssue, matchBrowseRow, type BrowseSearchRow } from "./browse.js";
import { issueKeys } from "./keys.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "123e4567-e89b-12d3-a456-426614174000";

function searchRow(overrides: Partial<BrowseSearchRow> = {}): BrowseSearchRow {
  return {
    name: "Fix login",
    id: ISSUE_ID,
    sequence_id: 42,
    project__identifier: "ACME",
    project_id: PROJECT_ID,
    workspace__slug: "acme",
    ...overrides,
  };
}

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

describe("matchBrowseRow", () => {
  it("matches the exact identifier-plus-sequence pair", () => {
    const rows = [searchRow(), searchRow({ project__identifier: "OTHER", sequence_id: 7 })];
    expect(matchBrowseRow(rows, "ACME-42")).toEqual(rows[0]);
  });

  it("rejects empty keys, prefixes and case drift", () => {
    const rows = [searchRow()];
    expect(matchBrowseRow(rows, "")).toBeNull();
    expect(matchBrowseRow(rows, "ACME-4")).toBeNull();
    expect(matchBrowseRow(rows, "acme-42")).toBeNull();
    expect(matchBrowseRow(rows, "OTHER-9")).toBeNull();
    expect(matchBrowseRow([], "ACME-42")).toBeNull();
  });
});

describe("loadBrowseIssue", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("resolves the key through search, then reads the detail", async () => {
    vi.mocked(searchWorkspace).mockResolvedValue({ results: { issue: [searchRow()] } });
    vi.mocked(getIssue).mockResolvedValue(issue());

    const resolution = await loadBrowseIssue("client" as never, "acme", "ACME-42");

    expect(searchWorkspace).toHaveBeenCalledWith("client", "acme", {
      search: "ACME-42",
      workspaceSearch: true,
      entities: ["issue"],
    });
    expect(getIssue).toHaveBeenCalledWith("client", "acme", PROJECT_ID, ISSUE_ID);
    expect(resolution.issue?.name).toBe("Fix login");
    expect(resolution.projectId).toBe(PROJECT_ID);
  });

  it("returns an empty resolution when no row matches", async () => {
    vi.mocked(searchWorkspace).mockResolvedValue({ results: {} });

    const resolution = await loadBrowseIssue("client" as never, "acme", "ACME-404");

    expect(resolution).toEqual({ issue: null, projectId: null });
    expect(getIssue).not.toHaveBeenCalled();
  });
});

describe("browseIssueQueryOptions", () => {
  it("keys the resolution by workspace and key", () => {
    const options = browseIssueQueryOptions("client" as never, "acme", "ACME-42");
    expect(options.queryKey).toEqual(issueKeys.browse("acme", "ACME-42"));
  });
});
