// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { SheetCellContext } from "./cells.js";
import { SheetPropertyCell, formatDayOrBlank, isEditableColumn } from "./cells.js";
import { SHEET_COLUMNS } from "./model.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";
const LABEL = "623e4567-e89b-12d3-a456-426614174005";
const MEMBER = "723e4567-e89b-12d3-a456-426614174006";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 2,
    attachment_count: 3,
    link_count: 1,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function column(key: string) {
  return SHEET_COLUMNS.find((entry) => entry.key === key)!;
}

function stubClient() {
  createAppClient("https://api.test", (async (url: string) => {
    let body: unknown = {};
    if (url.endsWith("/auth/get-csrf-token/")) body = { csrf_token: "csrf" };
    else if (url.includes("/states/")) {
      body = [
        {
          id: STATE,
          project_id: PROJECT,
          workspace_id: PROJECT,
          name: "In progress",
          color: "#123456",
          group: "started",
          default: false,
          description: "",
          sequence: 1,
        },
      ];
    } else if (url.includes("/cycles/")) {
      body = [{ id: CYCLE, project_id: PROJECT, name: "Cycle 1", status: "ACTIVE", is_favorite: false }];
    } else if (url.includes("/modules/")) {
      body = [{ id: MODULE, project_id: PROJECT, name: "Module 1", status: "active", is_favorite: false }];
    } else if (url.includes("/issue-labels/")) {
      body = [
        {
          id: LABEL,
          project_id: PROJECT,
          workspace_id: PROJECT,
          name: "Bug",
          color: "#ff0000",
          parent: null,
          sort_order: 1,
        },
      ];
    } else if (url.endsWith("/members/")) {
      body = [
        {
          id: MEMBER,
          role: 15,
          member: MEMBER,
          project: PROJECT,
          original_role: 15,
          created_at: "2026-01-01T00:00:00Z",
        },
      ];
    } else if (url.endsWith("/api/workspaces/acme/members/")) {
      body = [
        {
          id: MEMBER,
          member: {
            id: MEMBER,
            first_name: "Pat",
            last_name: "Member",
            avatar: "",
            avatar_url: null,
            display_name: "Pat Member",
            is_bot: false,
          },
          role: 20,
        },
      ];
    } else if (url.includes("/project-roles/")) body = { [PROJECT]: 20 };
    else if (url.includes("/workspace-members/me/")) body = { id: MEMBER, role: 20 };
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
}

function context(overrides: Partial<SheetCellContext> = {}): SheetCellContext {
  return {
    workspaceSlug: SLUG,
    projectId: PROJECT,
    editable: true,
    weekStart: 0,
    onUpdate: vi.fn(async () => undefined),
    onCycle: vi.fn(async () => undefined),
    onModules: vi.fn(async () => undefined),
    onOpenSubIssues: vi.fn(),
    isRowReadOnly: () => false,
    focusCell: vi.fn(),
    ...overrides,
  };
}

function renderCell(key: string, cellContext = context(), row = issue()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <table>
        <tbody>
          <tr>
            <SheetPropertyCell
              issue={row}
              column={column(key)}
              context={cellContext}
              rowIndex={0}
              columnIndex={1}
              onKeyDown={() => {}}
            />
          </tr>
        </tbody>
      </table>
    </QueryClientProvider>
  );
  return { ...result, context: cellContext, queryClient };
}

afterEach(() => {
  resetAppClient();
});

describe("SheetPropertyCell", () => {
  it("marks the spec's editable columns and leaves the rest read-only", () => {
    for (const key of [
      "state",
      "priority",
      "assignee",
      "start_date",
      "due_date",
      "estimate",
      "labels",
      "cycle",
      "modules",
    ]) {
      expect(isEditableColumn(key)).toBe(true);
    }
    for (const key of ["created_on", "updated_on", "link", "attachment_count", "sub_issue_count"]) {
      expect(isEditableColumn(key)).toBe(false);
    }
  });

  it("carries the row/column coordinates and an accessible name", () => {
    const { container } = renderCell("priority");
    const cell = container.querySelector("td")!;
    expect(cell.id).toBe(`sheet-cell-${ISSUE}-priority`);
    expect(cell).toHaveAttribute("tabindex", "0");
    expect(cell).toHaveAttribute("data-row-index", "0");
    expect(cell).toHaveAttribute("data-column-index", "1");
    expect(cell).toHaveAttribute("aria-label", "Priority for Fix login");
  });

  it("writes a priority immediately, then refocuses the cell", async () => {
    stubClient();
    const ctx = context();
    renderCell("priority", ctx);
    fireEvent.click(screen.getByRole("button", { name: /high/i }));
    fireEvent.click(screen.getByRole("option", { name: "Urgent" }));
    expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { priority: "urgent" });
    await vi.waitFor(() => expect(ctx.focusCell).toHaveBeenCalledWith(ISSUE, "priority", 1));
  });

  it("writes a state through the picker", async () => {
    stubClient();
    const ctx = context();
    renderCell("state", ctx);
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    await screen.findByRole("option", { name: /In progress/ });
    fireEvent.click(screen.getByRole("option", { name: /In progress/ }));
    expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { state_id: STATE });
    await vi.waitFor(() => expect(ctx.focusCell).toHaveBeenCalledWith(ISSUE, "state", 1));
  });

  it("keeps the due-date bound on a start-date write", async () => {
    stubClient();
    const ctx = context();
    renderCell("start_date", ctx, issue({ start_date: null, target_date: "2026-10-20" }));
    fireEvent.click(screen.getByTestId(`sheet-start-${ISSUE}-trigger`));
    fireEvent.click(screen.getByRole("button", { name: "October 15, 2026" }));
    expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { start_date: "2026-10-15", target_date: "2026-10-20" });
    await vi.waitFor(() => expect(ctx.focusCell).toHaveBeenCalledWith(ISSUE, "start_date", 1));
  });

  it("keeps the start-date bound on a due-date write", async () => {
    stubClient();
    const ctx = context();
    renderCell("due_date", ctx, issue({ start_date: "2026-10-10", target_date: null }));
    fireEvent.click(screen.getByTestId(`sheet-due-${ISSUE}-trigger`));
    const calendar = screen.getByTestId(`sheet-due-${ISSUE}-calendar`);
    expect(within(calendar).getByRole("button", { name: "October 9, 2026" })).toBeDisabled();
    fireEvent.click(within(calendar).getByRole("button", { name: "October 15, 2026" }));
    expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { start_date: "2026-10-10", target_date: "2026-10-15" });
    await vi.waitFor(() => expect(ctx.focusCell).toHaveBeenCalledWith(ISSUE, "due_date", 1));
  });

  it("flags an overdue due date", () => {
    stubClient();
    renderCell("due_date", context(), issue({ target_date: "2020-01-01" }));
    expect(screen.getByTestId(`sheet-due-${ISSUE}`).parentElement).toHaveClass("text-(--danger)");
  });

  it("updates cycle membership through the bridge, not the plain patch", async () => {
    stubClient();
    const ctx = context();
    renderCell("cycle", ctx, issue({ cycle_id: null }));
    fireEvent.click(screen.getByRole("button", { name: "No cycle" }));
    await screen.findByRole("option", { name: "Cycle 1" });
    fireEvent.click(screen.getByRole("option", { name: "Cycle 1" }));
    await vi.waitFor(() => expect(ctx.onCycle).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE }), CYCLE));
    expect(ctx.onUpdate).not.toHaveBeenCalled();
  });

  it("updates modules through their bridge", async () => {
    stubClient();
    const ctx = context();
    renderCell("modules", ctx, issue({ module_ids: [] }));
    fireEvent.click(screen.getByRole("button", { name: /modules/i }));
    await screen.findByRole("option", { name: /Module 1/ });
    fireEvent.click(screen.getByRole("option", { name: /Module 1/ }));
    await vi.waitFor(() =>
      expect(ctx.onModules).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE }), [MODULE])
    );
  });

  it("updates labels through the shared picker", async () => {
    stubClient();
    const ctx = context();
    renderCell("labels", ctx, issue({ label_ids: [] }));
    fireEvent.click(screen.getByTestId("label-picker-trigger"));
    await screen.findByTestId("label-picker-option");
    fireEvent.click(screen.getByTestId("label-picker-option"));
    await vi.waitFor(() => expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { label_ids: [LABEL] }));
  });

  it("renders timestamps, counts, and priority as text", () => {
    stubClient();
    const { unmount } = renderCell("created_on");
    expect(screen.getByText("Jan 2, 2026")).toBeInTheDocument();
    unmount();
    renderCell("updated_on");
    expect(screen.getByText("Feb 3, 2026")).toBeInTheDocument();
  });

  it("renders link and attachment counts", () => {
    const { unmount } = renderCell("link");
    expect(screen.getByText("1")).toBeInTheDocument();
    unmount();
    renderCell("attachment_count");
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("opens sub-issues from the count cell and disables an empty count", () => {
    const ctx = context();
    const { unmount } = renderCell("sub_issue_count", ctx);
    fireEvent.click(screen.getByRole("button", { name: "2 sub-work items" }));
    expect(ctx.onOpenSubIssues).toHaveBeenCalledWith(expect.objectContaining({ id: ISSUE }));
    unmount();
    renderCell("sub_issue_count", ctx, issue({ sub_issues_count: 0 }));
    expect(screen.getByRole("button", { name: "0 sub-work items" })).toBeDisabled();
  });

  it("renders read-only text for guests", () => {
    stubClient();
    renderCell("priority", context({ editable: false }));
    expect(screen.queryByRole("button", { name: /high/i })).not.toBeInTheDocument();
    expect(screen.getByText("high")).toBeInTheDocument();
  });

  it("renders read-only text for completed-cycle rows", () => {
    stubClient();
    renderCell("state", context({ isRowReadOnly: () => true }), issue());
    expect(screen.queryByRole("button", { name: /in progress/i })).not.toBeInTheDocument();
  });

  it("renders the remaining read-only value branches", () => {
    const { unmount } = renderCell("assignee", context({ editable: false }), issue({ assignee_ids: [MEMBER] }));
    expect(screen.getByText("72")).toBeInTheDocument();
    unmount();
    const priority = renderCell("priority", context({ editable: false }), issue({ priority: "none" }));
    expect(priority.container.querySelector("td")?.textContent).toBe("");
    priority.unmount();
    expect(formatDayOrBlank(null)).toBe("");
    expect(formatDayOrBlank("2026-10-15")).toBe("Oct 15, 2026");
  });
});
