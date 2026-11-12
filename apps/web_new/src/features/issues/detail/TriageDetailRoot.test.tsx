// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import type { Issue } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

vi.mock("./IssueDetailRoot.js", () => ({
  IssueDetailRoot: ({ issue, editable }: { issue: Issue; editable: boolean }) => (
    <div data-testid="issue-detail-root" data-editable={editable ? "true" : "false"}>
      {issue.name}
    </div>
  ),
}));

import { TriageDetailRoot } from "./TriageDetailRoot.js";

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Move me to the backlog",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    sequence_id: 4,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-17T12:00:00Z",
    updated_at: "2026-10-17T12:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Please triage</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

describe("TriageDetailRoot", () => {
  it("shows the decision strip beside a read-only work-item body", () => {
    render(
      <TriageDetailRoot
        workspaceSlug="acme"
        projectId="223e4567-e89b-12d3-a456-426614174001"
        issue={issue()}
        reference="ENG-4"
      />
    );
    expect(screen.getByTestId("issue-triage-root")).toHaveTextContent("Waiting for a triage decision on ENG-4.");
    expect(screen.getByTestId("issue-detail-root")).toHaveTextContent("Move me to the backlog");
    expect(screen.getByTestId("issue-detail-root")).toHaveAttribute("data-editable", "false");
  });

  it("keeps the decision actions visible but inert", () => {
    render(
      <TriageDetailRoot
        workspaceSlug="acme"
        projectId="223e4567-e89b-12d3-a456-426614174001"
        issue={issue()}
        reference="ENG-4"
      />
    );
    const accept = screen.getByRole("button", { name: "Accept" });
    const decline = screen.getByRole("button", { name: "Decline" });
    expect(accept).toBeDisabled();
    expect(decline).toBeDisabled();
    expect(accept).toHaveAttribute("title", "Triage decisions are made from the Intake screen");
    expect(decline).toHaveAttribute("title", "Triage decisions are made from the Intake screen");
  });
});
