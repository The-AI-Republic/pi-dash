// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline property controls on a child row (parity/specs/issues.md §15 +
// §17, ISS-177): each visible column owns one control and each choice
// writes exactly one field, the merged range writes both ends in one
// patch, hidden columns disappear, guests get disabled triggers, and the
// compact time cell marks an overdue child.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { SubIssueItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { defaultDisplayProperties, type SubIssueDisplayProperties } from "./rules.js";
import { SubIssuePropertyControls } from "./SubIssueProperties.js";
import type { SubIssueRowActions } from "./types.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "423e4567-e89b-12d3-a456-426614174003";

function row(overrides: Partial<SubIssueItem> = {}): SubIssueItem {
  return {
    id: CHILD,
    name: "Child one",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "medium",
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: PARENT,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    state_group: "started",
    ...overrides,
  };
}

function renderControls(
  options: {
    row?: SubIssueItem;
    properties?: SubIssueDisplayProperties;
    editable?: boolean;
    merged?: boolean;
    overdue?: boolean;
  } = {}
) {
  createAppClient(
    "https://api.test",
    (async (url: string) =>
      ({
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => "{}",
      }) as never) as never
  );
  const actions: SubIssueRowActions = { isBusy: () => false, write: vi.fn() };
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <SubIssuePropertyControls
        workspaceSlug={WS}
        projectId={PROJECT}
        row={options.row ?? row()}
        properties={options.properties ?? defaultDisplayProperties()}
        editable={options.editable ?? true}
        merged={options.merged ?? false}
        overdue={options.overdue ?? false}
        actions={actions}
      />
    </QueryClientProvider>
  );
  return { actions, ...view };
}

describe("SubIssuePropertyControls", () => {
  it("offers the row's priority as its own trigger", () => {
    renderControls();
    expect(screen.getByRole("button", { name: "Priority: medium" })).toBeInTheDocument();
  });

  it("writes only due_date from the due column", async () => {
    const { actions } = renderControls({ row: row({ target_date: "2026-10-20" }) });
    fireEvent.click(screen.getByTestId("sub-issue-due-date-clear"));
    expect(actions.write).toHaveBeenCalledWith(
      expect.objectContaining({ patch: { target_date: null }, field: "target_date" })
    );
  });

  it("writes both ends in one patch from the merged range", async () => {
    const { actions } = renderControls({
      row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }),
      merged: true,
    });
    fireEvent.click(screen.getByTestId("sub-issue-date-range-clear"));
    expect(actions.write).toHaveBeenCalledTimes(1);
    expect(actions.write).toHaveBeenCalledWith({
      row: expect.objectContaining({ id: CHILD }),
      patch: { start_date: null, target_date: null },
      field: "start_date",
    });
  });

  it("stops a control's click from reaching the row link", () => {
    const onRowClick = vi.fn();
    createAppClient(
      "https://api.test",
      (async (url: string) =>
        ({ status: 200, url, headers: { get: () => null }, text: async () => "{}" }) as never) as never
    );
    const actions: SubIssueRowActions = { isBusy: () => false, write: vi.fn() };
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <div onClick={onRowClick}>
          <SubIssuePropertyControls
            workspaceSlug={WS}
            projectId={PROJECT}
            row={row()}
            properties={defaultDisplayProperties()}
            editable
            merged={false}
            overdue={false}
            actions={actions}
          />
        </div>
      </QueryClientProvider>
    );
    const trigger = screen.getByRole("button", { name: "Priority: medium" });

    fireEvent.click(trigger);

    expect(onRowClick).not.toHaveBeenCalled();
  });

  it("hides every column the display turned off", () => {
    renderControls({
      properties: {
        ...defaultDisplayProperties(),
        state: false,
        priority: false,
        start_date: false,
        due_date: false,
        assignee: false,
      },
    });
    expect(screen.queryByRole("button", { name: /Priority/ })).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-start-date")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sub-issue-due-date")).not.toBeInTheDocument();
  });

  it("draws the two date columns separately when not merged", () => {
    renderControls({ row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }) });
    expect(screen.getByTestId("sub-issue-start-date")).toBeInTheDocument();
    expect(screen.getByTestId("sub-issue-due-date")).toBeInTheDocument();
  });

  it("disables every trigger for a guest", () => {
    renderControls({
      editable: false,
      row: row({ start_date: "2026-10-01", target_date: "2026-10-10" }),
    });
    expect(screen.getByTestId("sub-issue-start-date-trigger")).toBeDisabled();
    expect(screen.getByTestId("sub-issue-due-date-trigger")).toBeDisabled();
  });

  it("draws the grading tone on an overdue due column", () => {
    const overdue = renderControls({ row: row({ target_date: "2026-10-10" }), overdue: true });
    expect(screen.getByTestId("sub-issue-due-date").parentElement?.className).toContain("text-(--danger)");
    overdue.unmount();
    renderControls({ row: row({ target_date: "2026-10-10" }), overdue: false });
    expect(screen.getByTestId("sub-issue-due-date").parentElement?.className ?? "").not.toContain("text-(--danger)");
  });
});

afterEach(() => {
  resetAppClient();
});
