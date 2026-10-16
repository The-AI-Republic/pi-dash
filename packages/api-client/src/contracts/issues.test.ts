// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-issues list contract against a stub client (no backend).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { IssueListResponse, listArchivedIssues } from "./issues.js";

function stubClient(body: unknown, calls: { url?: string } = {}): ApiClient {
  return {
    getJson: (async (url: string) => {
      calls.url = url;
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
    const calls: { url?: string } = {};
    const response = await listArchivedIssues(stubClient(ENVELOPE, calls), "acme", "pid");

    expect(calls.url).toBe("/api/workspaces/acme/projects/pid/archived-issues/");
    expect(response.results).toHaveLength(1);
    expect(response.results[0]?.sequence_id).toBe(17);
  });

  it("accepts the envelope the stub client returns verbatim", () => {
    expect(() => IssueListResponse.parse(ENVELOPE)).not.toThrow();
  });
});
