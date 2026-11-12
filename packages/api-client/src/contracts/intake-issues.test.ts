// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake-issues contracts against a stub transport (no backend): the
// list envelope and query encoding, the detail shape, the create/update
// bodies and the delete status. Live-server proof lives in
// contract/intake-issues.contract.test.ts.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import {
  INTAKE_STATUS,
  IntakeIssueDetail,
  IntakeIssueListResponse,
  createIntakeIssue,
  deleteIntakeIssue,
  getIntakeIssue,
  intakeIssueListQueryParams,
  listIntakeIssues,
  updateIntakeIssue,
} from "./intake-issues.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const ENTRY = "323e4567-e89b-12d3-a456-426614174002";

function json(value: unknown, status = 200) {
  return recordingTransport((call) =>
    call.url.endsWith("/auth/get-csrf-token/")
      ? fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) })
      : fakeResponse({ status, body: JSON.stringify(value) })
  );
}

const SUMMARY = {
  id: ISSUE,
  name: "Intake probe",
  priority: "high",
  sequence_id: 7,
  project_id: PROJECT,
  created_at: "2026-10-20T06:32:06.646003Z",
  label_ids: [],
  created_by: "423e4567-e89b-12d3-a456-426614174003",
};

const ENVELOPE = {
  grouped_by: null,
  sub_grouped_by: null,
  total_count: 1,
  next_cursor: "10:1:0",
  prev_cursor: "10:0:0",
  next_page_results: true,
  prev_page_results: false,
  count: 1,
  total_pages: 2,
  total_results: 11,
  extra_stats: null,
  results: [
    {
      id: ENTRY,
      status: INTAKE_STATUS.pending,
      duplicate_to: null,
      snoozed_till: null,
      source: "IN_APP",
      issue: SUMMARY,
      created_by: "423e4567-e89b-12d3-a456-426614174003",
    },
  ],
};

const DETAIL_ISSUE = {
  id: ISSUE,
  name: "Intake probe",
  state_id: null,
  sort_order: 1,
  completed_at: null,
  estimate_point: null,
  priority: "high",
  start_date: null,
  target_date: null,
  sequence_id: 7,
  project_id: PROJECT,
  parent_id: null,
  cycle_id: null,
  module_ids: [],
  label_ids: [],
  assignee_ids: [],
  sub_issues_count: null,
  attachment_count: null,
  link_count: null,
  created_at: "2026-10-20T06:32:06.646003Z",
  updated_at: "2026-10-20T06:32:06.646003Z",
  created_by: null,
  updated_by: null,
  is_draft: false,
  archived_at: null,
  complexity_score: 0,
  agent_executor: null,
  is_synced: false,
  description_html: "<p>probe</p>",
  is_subscribed: false,
  agent_ticker: null,
  agent_status: null,
  relations_summary: { blocked_by: [], blocking: [] },
  has_open_blockers: false,
  is_intake: true,
};

const DETAIL = {
  id: ENTRY,
  status: INTAKE_STATUS.pending,
  duplicate_to: null,
  snoozed_till: null,
  duplicate_issue_detail: null,
  source: "IN_APP",
  issue: DETAIL_ISSUE,
};

describe("intake list query params", () => {
  it("joins multi-value filters and carries ranges verbatim", () => {
    expect(
      intakeIssueListQueryParams({
        statuses: [INTAKE_STATUS.pending, INTAKE_STATUS.snoozed],
        priorities: ["high", "urgent"],
        assignees: ["u-1"],
        createdBy: ["u-2", "u-3"],
        labels: ["l-1"],
        createdAt: "2026-10-01;after,2026-10-31;before",
        updatedAt: "2026-10-01;after",
        orderBy: "-issue__created_at",
        cursor: "10:1:0",
        perPage: 10,
      })
    ).toEqual({
      status: "-2,0",
      priority: "high,urgent",
      assignees: "u-1",
      created_by: "u-2,u-3",
      labels: "l-1",
      created_at: "2026-10-01;after,2026-10-31;before",
      updated_at: "2026-10-01;after",
      order_by: "-issue__created_at",
      cursor: "10:1:0",
      per_page: 10,
    });
  });

  it("omits every filter when the query is empty", () => {
    const params = intakeIssueListQueryParams({});
    expect(Object.values(params).every((value) => value === undefined)).toBe(true);
  });
});

describe("listIntakeIssues", () => {
  it("reads the cursor envelope and encodes the screen's query surface", async () => {
    const transport = json(ENVELOPE);
    const client = createClient({ baseUrl: "https://api.test", transport });

    const parsed = await listIntakeIssues(client, SLUG, PROJECT, {
      statuses: [INTAKE_STATUS.pending],
      priorities: ["high"],
      orderBy: "issue__sequence_id",
      perPage: 10,
    });

    expect(parsed.total_results).toBe(11);
    expect(parsed.results).toHaveLength(1);
    expect(parsed.results[0]?.issue.sequence_id).toBe(7);
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(url.pathname).toBe(`/api/workspaces/${SLUG}/projects/${PROJECT}/intake-issues/`);
    expect(url.searchParams.get("status")).toBe("-2");
    expect(url.searchParams.get("priority")).toBe("high");
    expect(url.searchParams.get("order_by")).toBe("issue__sequence_id");
    expect(url.searchParams.get("per_page")).toBe("10");
  });

  it("accepts the envelope the stub transport returns verbatim", () => {
    expect(() => IntakeIssueListResponse.parse(ENVELOPE)).not.toThrow();
  });
});

describe("getIntakeIssue", () => {
  it("reads one request through the work-item id", async () => {
    const transport = json(DETAIL);
    const client = createClient({ baseUrl: "https://api.test", transport });

    const parsed = await getIntakeIssue(client, SLUG, PROJECT, ISSUE);

    expect(parsed.issue.is_intake).toBe(true);
    expect(parsed.duplicate_issue_detail).toBeNull();
    expect(transport.calls[0]?.url).toBe(
      `https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/intake-issues/${ISSUE}/`
    );
  });

  it("accepts a duplicate target summary without label_ids", () => {
    const { label_ids: _dropped, ...summary } = SUMMARY;
    expect(() =>
      IntakeIssueDetail.parse({ ...DETAIL, status: INTAKE_STATUS.duplicate, duplicate_issue_detail: summary })
    ).not.toThrow();
  });

  it("accepts the detail without the keys the intake viewset never annotates", () => {
    const {
      cycle_id: _cycle,
      module_ids: _modules,
      sub_issues_count: _sub,
      attachment_count: _attachments,
      link_count: _links,
      is_subscribed: _subscribed,
      is_intake: _intake,
      ...sparse
    } = DETAIL_ISSUE;
    expect(() => IntakeIssueDetail.parse({ ...DETAIL, issue: sparse })).not.toThrow();
  });
});

describe("createIntakeIssue", () => {
  it("posts source plus the nested issue and reads the detail answer", async () => {
    const transport = json(DETAIL);
    const client = createClient({ baseUrl: "https://api.test", transport });

    const parsed = await createIntakeIssue(client, SLUG, PROJECT, {
      source: "IN_APP",
      issue: { name: "Intake probe", priority: "high", target_date: "2026-11-01" },
    });

    expect(parsed.issue.name).toBe("Intake probe");
    const call = transport.calls.find((entry) => entry.url.includes("/intake-issues/"));
    expect(call?.init.method).toBe("POST");
    expect(call?.init.body).toContain("IN_APP");
    expect(call?.init.body).toContain("Intake probe");
  });
});

describe("updateIntakeIssue", () => {
  it("patches intake fields at top level and work-item fields nested", async () => {
    const transport = json({ ...DETAIL, status: INTAKE_STATUS.snoozed });
    const client = createClient({ baseUrl: "https://api.test", transport });

    const parsed = await updateIntakeIssue(client, SLUG, PROJECT, ISSUE, {
      status: INTAKE_STATUS.snoozed,
      snoozed_till: "2026-11-01T00:00:00Z",
      issue: { priority: "urgent" },
    });

    expect(parsed.status).toBe(INTAKE_STATUS.snoozed);
    const call = transport.calls.find((entry) => entry.url.includes(`/intake-issues/${ISSUE}/`));
    expect(call?.init.method).toBe("PATCH");
    expect(call?.init.body).toContain("snoozed_till");
    expect(call?.init.body).toContain("urgent");
  });
});

describe("deleteIntakeIssue", () => {
  it("deletes one request and resolves the 204", async () => {
    const transport = json(null, 204);
    const client = createClient({ baseUrl: "https://api.test", transport });

    const status = await deleteIntakeIssue(client, SLUG, PROJECT, ISSUE);

    expect(status).toBe(204);
    const call = transport.calls.find((entry) => entry.url.includes(`/intake-issues/${ISSUE}/`));
    expect(call?.init.method).toBe("DELETE");
  });
});
