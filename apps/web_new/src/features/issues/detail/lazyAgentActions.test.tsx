// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The lazy agent-controls entry (NEWFRONT-867): the peek mounts this, so
// the runners dispatch code stays out of the initial chain. The entry
// holds its placeholder until the agent chunk resolves, then the row.
import { render, screen } from "@testing-library/react";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { IssueAgentActionsEntry } from "./lazyAgentActions.js";

const chunkGate = vi.hoisted(() => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  return {
    gate,
    release: () => release(),
  };
});

vi.mock("./IssueAgentActions.js", async () => {
  await chunkGate.gate;
  const stub = (): React.ReactElement => <div data-testid="issue-agent-actions" />;
  return { IssueAgentActions: stub };
});

function issueRow(): Issue {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Ship it",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 12,
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
    description_html: "<p>Detail</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

describe("IssueAgentActionsEntry", () => {
  it("holds its placeholder until the agent chunk resolves, then renders the row", async () => {
    render(
      <IssueAgentActionsEntry
        workspaceSlug="acme"
        workspaceId="a23e4567-e89b-12d3-a456-426614174010"
        projectId="223e4567-e89b-12d3-a456-426614174001"
        issue={issueRow()}
        disabled={false}
      />
    );
    expect(screen.getByText("Loading agent actions…")).toBeInTheDocument();
    expect(screen.queryByTestId("issue-agent-actions")).not.toBeInTheDocument();
    chunkGate.release();
    expect(await screen.findByTestId("issue-agent-actions")).toBeInTheDocument();
    expect(screen.queryByText("Loading agent actions…")).not.toBeInTheDocument();
  });
});
