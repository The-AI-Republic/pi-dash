// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-issues contracts against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  IssueListResponse,
  getArchivedIssue,
  listArchivedIssues,
  listGroupedArchivedIssues,
  listSubGroupedArchivedIssues,
} from "./issues.js";

interface StubCalls {
  url?: string;
  query?: Record<string, string | number | boolean | undefined | null> | undefined;
}

function stubClient(body: unknown, calls: StubCalls = {}): ApiClient {
  return {
    getJson: (async (
      url: string,
      options?: {
        query?: Record<string, string | number | boolean | undefined | null>;
      }
    ) => {
      calls.url = url;
      calls.query = options?.query;
      return body;
    }) as ApiClient["getJson"],
    parse: ((schema: { parse: (value: unknown) => never }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
  } as ApiClient;
}

const ENVELOPE = {
  grouped_by: null,
  sub_grouped_by: null,
  total_count: 1,
  next_cursor: "",
  prev_cursor: "",
  next_page_results: false,
  prev_page_results: false,
  count: 1,
  total_pages: 1,
  total_results: 1,
  extra_stats: {},
  results: [
    {
      id: "123e4567-e89b-12d3-a456-426614174000",
      name: "Archived probe",
      state_id: null,
      sort_order: 1,
      completed_at: null,
      estimate_point: null,
      priority: "high",
      start_date: null,
      target_date: null,
      sequence_id: 17,
      project_id: "223e4567-e89b-12d3-a456-426614174001",
      parent_id: null,
      cycle_id: null,
      module_ids: [],
      label_ids: [],
      assignee_ids: [],
      sub_issues_count: null,
      attachment_count: null,
      link_count: null,
      created_at: "2026-10-16T06:32:06.646003Z",
      updated_at: "2026-10-16T15:32:05.479661+09:00",
      created_by: null,
      updated_by: null,
      is_draft: false,
      archived_at: "2026-10-16",
    },
  ],
};

describe("listArchivedIssues", () => {
  it("reads the archived list through the shared list envelope", async () => {
    const calls: StubCalls = {};
    const response = await listArchivedIssues(stubClient(ENVELOPE, calls), "acme", "pid");

    expect(calls.url).toBe("/api/workspaces/acme/projects/pid/archived-issues/");
    expect(response.results).toHaveLength(1);
    expect(response.results[0]?.sequence_id).toBe(17);
  });

  it("accepts the envelope the stub client returns verbatim", () => {
    expect(() => IssueListResponse.parse(ENVELOPE)).not.toThrow();
  });

  it("sends the live list query: paging, order and the filters expression", async () => {
    const calls: StubCalls = {};
    await listArchivedIssues(stubClient(ENVELOPE, calls), "acme", "pid", {
      perPage: 25,
      cursor: "25:1:0",
      orderBy: "-created_at",
      filters: { filters: JSON.stringify({ and: [{ priority__in: "urgent" }] }) },
    });

    expect(calls.url).toBe("/api/workspaces/acme/projects/pid/archived-issues/");
    expect(calls.query).toMatchObject({
      per_page: 25,
      cursor: "25:1:0",
      order_by: "-created_at",
      filters: JSON.stringify({ and: [{ priority__in: "urgent" }] }),
    });
  });
});

const GROUPED_ENVELOPE = {
  ...ENVELOPE,
  grouped_by: "priority",
  results: {
    urgent: {
      results: [{ ...ENVELOPE.results[0], state__group: "completed" }],
      total_results: 1,
    },
  },
};

const SUB_GROUPED_ENVELOPE = {
  ...ENVELOPE,
  grouped_by: "priority",
  sub_grouped_by: "state__group",
  results: {
    urgent: {
      results: {
        completed: {
          results: [{ ...ENVELOPE.results[0], state__group: "completed" }],
          total_results: 1,
        },
      },
      total_results: 1,
    },
  },
};

describe("listGroupedArchivedIssues", () => {
  it("sends group_by and parses the bucket envelope", async () => {
    const calls: StubCalls = {};
    const response = await listGroupedArchivedIssues(stubClient(GROUPED_ENVELOPE, calls), "acme", "pid", {
      groupBy: "priority",
    });

    expect(calls.url).toBe("/api/workspaces/acme/projects/pid/archived-issues/");
    expect(calls.query).toMatchObject({ group_by: "priority" });
    expect(response.grouped_by).toBe("priority");
    expect(response.results["urgent"]?.total_results).toBe(1);
  });
});

describe("listSubGroupedArchivedIssues", () => {
  it("sends group_by with sub_group_by and parses the nested buckets", async () => {
    const calls: StubCalls = {};
    const response = await listSubGroupedArchivedIssues(stubClient(SUB_GROUPED_ENVELOPE, calls), "acme", "pid", {
      groupBy: "priority",
      subGroupBy: "state__group",
    });

    expect(calls.url).toBe("/api/workspaces/acme/projects/pid/archived-issues/");
    expect(calls.query).toMatchObject({ group_by: "priority", sub_group_by: "state__group" });
    expect(response.results["urgent"]?.results["completed"]?.total_results).toBe(1);
  });
});

// The archived retrieve omits the annotation-backed keys (cycle,
// memberships, counts), so the fixture drops them: the schema must
// parse without them.
const {
  cycle_id: _cycleId,
  module_ids: _moduleIds,
  label_ids: _labelIds,
  assignee_ids: _assigneeIds,
  sub_issues_count: _subIssuesCount,
  attachment_count: _attachmentCount,
  link_count: _linkCount,
  ...ARCHIVED_ROW
} = ENVELOPE.results[0] ?? {};
void _cycleId;
void _moduleIds;
void _labelIds;
void _assigneeIds;
void _subIssuesCount;
void _attachmentCount;
void _linkCount;

const ARCHIVED_DETAIL = {
  ...ARCHIVED_ROW,
  complexity_score: 0,
  agent_executor: null,
  is_synced: false,
  description_html: "<p>Archived probe</p>",
  is_subscribed: true,
  agent_ticker: null,
  agent_status: null,
  relations_summary: { blocked_by: [], blocking: [] },
  has_open_blockers: false,
};

describe("getArchivedIssue", () => {
  it("reads the archive-scoped path with the shared detail shape", async () => {
    const calls: StubCalls = {};
    const detail = await getArchivedIssue(
      stubClient(ARCHIVED_DETAIL, calls),
      "acme",
      "pid",
      "123e4567-e89b-12d3-a456-426614174000"
    );

    expect(calls.url).toBe("/api/workspaces/acme/projects/pid/issues/123e4567-e89b-12d3-a456-426614174000/archive/");
    expect(detail.id).toBe("123e4567-e89b-12d3-a456-426614174000");
    expect(detail.archived_at).toBe("2026-10-16");
    expect(detail.is_subscribed).toBe(true);
  });
});
