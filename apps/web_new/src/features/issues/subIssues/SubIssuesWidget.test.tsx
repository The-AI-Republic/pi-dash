// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail widget root (parity/specs/issues.md §15, ISS-173): the five
// action entries always render in order, only the sub-work-item entry is
// live in this slice and only for members, its menu opens the create and
// add-existing dialogs through the section handle, the section receives
// the kind's label, and later widgets keep their slot order.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const handleCreate = vi.fn();
const handleExisting = vi.fn();
const sectionProps = vi.fn();

vi.mock("./SubIssueSection.js", () => ({
  SubIssueSection: React.forwardRef(function MockSection(
    props: Record<string, unknown>,
    ref: React.ForwardedRef<{ openCreate: () => void; openExisting: () => void }>
  ) {
    sectionProps(props);
    React.useImperativeHandle(ref, () => ({ openCreate: handleCreate, openExisting: handleExisting }));
    return <div data-testid="mock-sub-issues-section">{String(props["heading"])}</div>;
  }),
}));

import { SubIssuesWidget, WIDGET_ACTION_LABELS } from "./SubIssuesWidget.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const PARENT = "323e4567-e89b-12d3-a456-426614174002";

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: PARENT,
    name: "Parent work",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    sequence_id: 4,
    project_id: PROJECT,
    parent_id: null,
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
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function renderWidget(
  options: {
    issue?: Issue;
    editable?: boolean;
    canWrite?: boolean;
    extraSections?: React.ComponentProps<typeof SubIssuesWidget>["extraSections"];
    onAttach?: React.ComponentProps<typeof SubIssuesWidget>["onAttach"];
    runAIAction?: React.ReactNode;
  } = {}
) {
  return render(
    <SubIssuesWidget
      workspaceSlug="acme"
      issue={options.issue ?? issue()}
      project={undefined}
      editable={options.editable ?? true}
      projectIdentifier="ATL"
      canWrite={options.canWrite ?? true}
      onOpen={vi.fn()}
      createProjects={[]}
      extraSections={options.extraSections}
      onAttach={options.onAttach}
      runAIAction={options.runAIAction}
    />
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("SubIssuesWidget", () => {
  it("renders every action entry in order", () => {
    renderWidget();
    for (const label of WIDGET_ACTION_LABELS) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    expect(WIDGET_ACTION_LABELS).toEqual([
      "Add sub-work item",
      "Add relation",
      "Add link",
      "Attach",
      "Manually Run AI",
    ]);
  });

  it("fills the Manually Run AI slot with the supplied mount", async () => {
    const onRun = vi.fn();
    renderWidget({
      runAIAction: (
        <button type="button" data-testid="issue-run-ai" onClick={onRun}>
          Manually Run AI
        </button>
      ),
    });
    const mount = screen.getByTestId("issue-run-ai");
    expect(mount).toBeEnabled();
    expect(mount).toHaveTextContent("Manually Run AI");
    await userEvent.setup().click(mount);
    expect(onRun).toHaveBeenCalledTimes(1);
  });

  it("keeps the default disabled Run AI entry when no mount is supplied", () => {
    renderWidget();
    expect(screen.getByRole("button", { name: "Manually Run AI" })).toBeDisabled();
    expect(screen.queryByTestId("issue-run-ai")).not.toBeInTheDocument();
  });

  it("leaves only the sub-work-item entry live for a member", () => {
    renderWidget();
    expect(screen.getByRole("button", { name: "Add sub-work item" })).toBeEnabled();
    for (const label of ["Add relation", "Add link", "Attach", "Manually Run AI"] as const) {
      expect(screen.getByRole("button", { name: label })).toBeDisabled();
    }
  });

  it("disables every entry for a guest", () => {
    renderWidget({ canWrite: false });
    for (const label of WIDGET_ACTION_LABELS) {
      expect(screen.getByRole("button", { name: label })).toBeDisabled();
    }
  });

  it("leaves the Attach entry shut without a picker", () => {
    renderWidget();
    expect(screen.getByRole("button", { name: "Attach" })).toBeDisabled();
  });

  it("opens the attachment picker from the Attach entry", async () => {
    const onAttach = vi.fn();
    renderWidget({ onAttach });
    const attach = screen.getByRole("button", { name: "Attach" });
    expect(attach).toBeEnabled();
    await userEvent.setup().click(attach);
    expect(onAttach).toHaveBeenCalledTimes(1);
  });

  it("keeps the Attach entry shut for a guest even with a picker", () => {
    renderWidget({ onAttach: vi.fn(), canWrite: false });
    expect(screen.getByRole("button", { name: "Attach" })).toBeDisabled();
  });

  it("disables every entry when the detail is not editable", () => {
    renderWidget({ editable: false });
    for (const label of WIDGET_ACTION_LABELS) {
      expect(screen.getByRole("button", { name: label })).toBeDisabled();
    }
  });

  it("opens create new from the action menu", async () => {
    renderWidget();
    await userEvent.setup().click(screen.getByRole("button", { name: "Add sub-work item" }));
    await userEvent.setup().click(await screen.findByRole("menuitem", { name: "Create new" }));
    expect(handleCreate).toHaveBeenCalledTimes(1);
    expect(handleExisting).not.toHaveBeenCalled();
  });

  it("opens add existing from the action menu", async () => {
    renderWidget();
    await userEvent.setup().click(screen.getByRole("button", { name: "Add sub-work item" }));
    await userEvent.setup().click(await screen.findByRole("menuitem", { name: "Add existing" }));
    expect(handleExisting).toHaveBeenCalledTimes(1);
    expect(handleCreate).not.toHaveBeenCalled();
  });

  it("titles the section sub-work items for an ordinary parent", () => {
    renderWidget();
    expect(screen.getByTestId("mock-sub-issues-section")).toHaveTextContent("Sub-work items");
  });

  it("titles the section work items for an epic", () => {
    renderWidget({ issue: issue({ is_epic: true }) });
    expect(screen.getByTestId("mock-sub-issues-section")).toHaveTextContent("Work items");
  });

  it("passes the workspace, parent and write standing to the section", () => {
    renderWidget();
    expect(sectionProps).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceSlug: "acme",
        issue: expect.objectContaining({ id: PARENT }),
        editable: true,
        canWrite: true,
        projectIdentifier: "ATL",
      })
    );
  });

  it("keeps extra widget slots under the sub-issues section in the given order", () => {
    renderWidget({
      extraSections: [
        { label: "Links", node: <p>links body</p> },
        { label: "Attachments", node: <p>attachments body</p> },
      ],
    });
    const root = screen.getByTestId("issue-detail-widgets");
    const labels = [...root.querySelectorAll("[data-widget-section]")].map((node) =>
      node.getAttribute("data-widget-section")
    );
    expect(labels).toEqual(["Links", "Attachments"]);
    expect(screen.getByText("links body")).toBeInTheDocument();
    expect(screen.getByText("attachments body")).toBeInTheDocument();
  });
});
