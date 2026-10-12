// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { Issue } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the detail content plus link target.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: { params?: Record<string, string>; children?: React.ReactNode }) => (
    <a href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}>{props.children}</a>
  ),
}));

import { BrowseDetail, BrowseUnknownKey } from "./BrowseDetail.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function detail(overrides: Partial<Issue> = {}): Issue {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
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

describe("BrowseDetail", () => {
  it("renders the key, the name and a way back into the project", () => {
    render(<BrowseDetail issue={detail()} issueKey="ACME-42" workspaceSlug="acme" projectId={PROJECT_ID} />);

    expect(screen.getByRole("heading", { name: "Fix login" })).not.toBeNull();
    expect(screen.getByText("ACME-42")).not.toBeNull();
    expect(screen.queryByText(/archived/)).toBeNull();
    expect(screen.getByLabelText("Work item detail")).toHaveAttribute("data-archived", "false");
    const back = screen.getByRole("link", { name: "Back to the project" });
    expect(back).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/issues`);
  });

  it("marks the archived state when the item carries an archived timestamp", () => {
    render(
      <BrowseDetail
        issue={detail({ archived_at: "2024-03-04T05:06:07Z" })}
        issueKey="ACME-42"
        workspaceSlug="acme"
        projectId={PROJECT_ID}
      />
    );
    expect(screen.getByText("This work item is archived.")).not.toBeNull();
    expect(screen.getByLabelText("Work item detail")).toHaveAttribute("data-archived", "true");
  });
});

describe("BrowseUnknownKey", () => {
  it("names the missing key without rendering detail", () => {
    render(<BrowseUnknownKey issueKey="ACME-404" />);
    expect(screen.getByRole("heading", { name: "No work item matches this address" })).not.toBeNull();
    expect(screen.getByText(/ACME-404/)).not.toBeNull();
  });
});
