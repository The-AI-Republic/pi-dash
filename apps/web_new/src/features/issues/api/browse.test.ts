// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", () => ({
  getIssue: vi.fn(),
  listArchivedIssues: vi.fn(),
  listProjectDetails: vi.fn(),
  searchWorkspace: vi.fn(),
}));

import {
  getIssue,
  listArchivedIssues,
  listProjectDetails,
  searchWorkspace,
  type Issue,
  type IssueListItem,
} from "@pidash/api-client";

import {
  browseIssueQueryOptions,
  loadBrowseIssue,
  matchBrowseRow,
  parseBrowseKey,
  type BrowseSearchRow,
} from "./browse.js";
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

describe("parseBrowseKey", () => {
  it("splits on the last dash", () => {
    expect(parseBrowseKey("ACME-42")).toEqual({ identifier: "ACME", sequenceId: 42 });
    expect(parseBrowseKey("AC-ME-42")).toEqual({ identifier: "AC-ME", sequenceId: 42 });
  });

  it("rejects keys without a numeric tail", () => {
    expect(parseBrowseKey("")).toBeNull();
    expect(parseBrowseKey("ACME")).toBeNull();
    expect(parseBrowseKey("ACME-")).toBeNull();
    expect(parseBrowseKey("-42")).toBeNull();
    expect(parseBrowseKey("ACME-4x")).toBeNull();
  });
});

describe("loadBrowseIssue", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function archivedRow(overrides: Partial<IssueListItem> = {}): IssueListItem {
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
      archived_at: "2024-03-04",
      ...overrides,
    };
  }

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
    expect(listProjectDetails).not.toHaveBeenCalled();
    expect(listArchivedIssues).not.toHaveBeenCalled();
  });

  it("falls back to the archived list when search misses an archived key", async () => {
    vi.mocked(searchWorkspace).mockResolvedValue({ results: {} });
    vi.mocked(listProjectDetails).mockResolvedValue([{ id: PROJECT_ID, identifier: "ACME" } as never]);
    vi.mocked(listArchivedIssues).mockResolvedValue({ results: [archivedRow()] } as never);
    vi.mocked(getIssue).mockResolvedValue(issue({ archived_at: "2024-03-04" }));

    const resolution = await loadBrowseIssue("client" as never, "acme", "ACME-42");

    expect(listProjectDetails).toHaveBeenCalledWith("client", "acme");
    expect(listArchivedIssues).toHaveBeenCalledWith("client", "acme", PROJECT_ID);
    expect(getIssue).toHaveBeenCalledWith("client", "acme", PROJECT_ID, ISSUE_ID);
    expect(resolution.issue?.archived_at).toBe("2024-03-04");
    expect(resolution.projectId).toBe(PROJECT_ID);
  });

  it("returns an empty resolution when no row matches", async () => {
    vi.mocked(searchWorkspace).mockResolvedValue({ results: {} });
    vi.mocked(listProjectDetails).mockResolvedValue([{ id: PROJECT_ID, identifier: "ACME" } as never]);
    vi.mocked(listArchivedIssues).mockResolvedValue({ results: [] } as never);

    const resolution = await loadBrowseIssue("client" as never, "acme", "ACME-404");

    expect(resolution).toEqual({ issue: null, projectId: null });
    expect(getIssue).not.toHaveBeenCalled();
  });

  it("skips the archived fallback for malformed keys and unknown projects", async () => {
    vi.mocked(searchWorkspace).mockResolvedValue({ results: {} });
    vi.mocked(listProjectDetails).mockResolvedValue([{ id: PROJECT_ID, identifier: "ACME" } as never]);

    expect(await loadBrowseIssue("client" as never, "acme", "no-key-here")).toEqual({
      issue: null,
      projectId: null,
    });
    expect(listProjectDetails).not.toHaveBeenCalled();

    expect(await loadBrowseIssue("client" as never, "acme", "OTHER-9")).toEqual({
      issue: null,
      projectId: null,
    });
    expect(listArchivedIssues).not.toHaveBeenCalled();
    expect(getIssue).not.toHaveBeenCalled();
  });
});

describe("browseIssueQueryOptions", () => {
  it("keys the resolution by workspace and key", () => {
    const options = browseIssueQueryOptions("client" as never, "acme", "ACME-42");
    expect(options.queryKey).toEqual(issueKeys.browse("acme", "ACME-42"));
  });
});
