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
import { edition } from "../../../core/edition/index.js";
import { workspaceScope } from "../../../core/query/index.js";
import { WORKFLOW_FLAG } from "../edition/flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../edition/providers.js";
import type { SheetCellContext } from "./cells.js";
import { SheetPropertyCell, formatDayOrBlank, isEditableColumn, isOverdue } from "./cells.js";
import { SHEET_COLUMNS } from "./model.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";
const LABEL = "623e4567-e89b-12d3-a456-426614174005";
const MEMBER = "723e4567-e89b-12d3-a456-426614174006";
const ESTIMATE = "823e4567-e89b-12d3-a456-426614174007";
const DONE = "923e4567-e89b-12d3-a456-426614174008";

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

function stubClient(stateGroup = "started", cycleStatus = "ACTIVE", extraState = false) {
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
          group: stateGroup,
          default: false,
          description: "",
          sequence: 1,
        },
        ...(extraState
          ? [
              {
                id: DONE,
                project_id: PROJECT,
                workspace_id: PROJECT,
                name: "Done",
                color: "#2f9e5b",
                group: "completed",
                default: false,
                description: "",
                sequence: 2,
              },
            ]
          : []),
      ];
    } else if (url.includes("/cycles/")) {
      body = [{ id: CYCLE, project_id: PROJECT, name: "Cycle 1", status: cycleStatus, is_favorite: false }];
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
    } else if (url === "https://api.test/api/workspaces/acme/members/") {
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
    } else if (url.includes("/project-roles/")) body = { [PROJECT]: 20 };
    else if (url.includes("/estimates/")) {
      body = [
        {
          id: ESTIMATE,
          workspace: PROJECT,
          project: PROJECT,
          name: "Points",
          description: "",
          type: "points",
          last_used: true,
          points: [{ id: ESTIMATE, key: 1, value: "3", description: "" }],
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
          created_by: null,
          updated_by: null,
        },
      ];
    } else if (url.includes("/workspace-members/me/")) body = { id: MEMBER, role: 20 };
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
  resetEditionIssuesProviders();
  delete edition.flags[WORKFLOW_FLAG];
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

  it("keeps every state open in the community build (ISS-233)", async () => {
    stubClient();
    registerEditionIssuesProviders({
      workflowRules: { refusal: () => ({ reason: "no rule should be asked" }) },
    });
    renderCell("state", context());
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    const option = await screen.findByRole("option", { name: /In progress/ });
    expect(option).toBeEnabled();
  });

  it("greys a state the rules refuse from the row's own state (ISS-233)", async () => {
    stubClient("started", "ACTIVE", true);
    edition.flags[WORKFLOW_FLAG] = true;
    registerEditionIssuesProviders({
      workflowRules: {
        refusal: ({ toStateId }) => (toStateId === DONE ? { reason: "Review the fix first" } : null),
      },
    });
    const ctx = context();
    renderCell("state", ctx, issue({ state_id: STATE }));
    fireEvent.click(screen.getByRole("button", { name: "Select state" }));
    const refused = await screen.findByRole("option", { name: /Done/ });
    expect(refused).toBeDisabled();
    expect(refused).toHaveTextContent("Review the fix first");
    expect(screen.getByRole("option", { name: /In progress/ })).toBeEnabled();
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

  it("writes a start date with the existing due date carried along", async () => {
    stubClient();
    const ctx = context();
    renderCell("start_date", ctx, issue({ start_date: null, target_date: null }));
    fireEvent.click(screen.getByTestId(`sheet-start-${ISSUE}-trigger`));
    fireEvent.click(screen.getByRole("button", { name: "October 15, 2026" }));
    expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { start_date: "2026-10-15", target_date: null });
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

  it("flags an overdue due date", async () => {
    stubClient();
    const rendered = renderCell("due_date", context(), issue({ target_date: "2020-01-01" }));
    await vi.waitFor(() =>
      expect(rendered.queryClient.getQueryData(["ws", SLUG, "pickers", "states", PROJECT])).toBeDefined()
    );
    await vi.waitFor(() =>
      expect(screen.getByTestId(`sheet-due-${ISSUE}`).parentElement).toHaveClass("text-(--danger)")
    );
  });

  it("does not flag overdue dates on completed or cancelled work", async () => {
    stubClient("completed");
    const completed = renderCell("due_date", context(), issue({ target_date: "2020-01-01" }));
    await vi.waitFor(() =>
      expect(completed.queryClient.getQueryData(["ws", SLUG, "pickers", "states", PROJECT])).toBeDefined()
    );
    expect(screen.getByTestId(`sheet-due-${ISSUE}`).parentElement).not.toHaveClass("text-(--danger)");
    completed.unmount();
    stubClient("cancelled");
    const cancelled = renderCell("due_date", context(), issue({ target_date: "2020-01-01" }));
    await vi.waitFor(() =>
      expect(cancelled.queryClient.getQueryData(["ws", SLUG, "pickers", "states", PROJECT])).toBeDefined()
    );
    expect(screen.getByTestId(`sheet-due-${ISSUE}`).parentElement).not.toHaveClass("text-(--danger)");
    expect(isOverdue("2020-01-01", "completed", "2026-10-18")).toBe(false);
    expect(isOverdue("2020-01-01", "cancelled", "2026-10-18")).toBe(false);
    expect(isOverdue("2020-01-01", "started", "2026-10-18")).toBe(true);
    expect(isOverdue("2020-01-01", undefined, "2026-10-18")).toBe(false);
    expect(isOverdue(null, "started", "2026-10-18")).toBe(false);
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
    const empty = screen.getByRole("button", { name: "0 sub-work items" });
    expect(empty).toHaveAttribute("aria-disabled", "true");
    expect(empty).not.toBeDisabled();
    fireEvent.click(empty);
    expect(ctx.onOpenSubIssues).toHaveBeenCalledTimes(1);
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

  it("renders every frozen column as a triggerless value with a None fallback", async () => {
    stubClient();
    const frozen = context({ isRowReadOnly: () => true });
    const row = issue({ cycle_id: CYCLE, label_ids: [LABEL], module_ids: [MODULE], assignee_ids: [MEMBER] });
    const { unmount } = renderCell("priority", frozen, row);
    expect(screen.getByText("High")).toBeInTheDocument();
    unmount();

    const kinds = {
      state: "state",
      assignee: "assignee",
      labels: "labels",
      modules: "module",
      cycle: "cycle",
      estimate: "estimate",
    };
    for (const [key, kind] of Object.entries(kinds)) {
      const rendered = renderCell(key, frozen, row);
      await vi.waitFor(() =>
        expect(rendered.container.querySelector(`[data-readonly-property="${kind}"]`), key).not.toBeNull()
      );
      expect(within(rendered.container).queryByRole("button")).not.toBeInTheDocument();
      rendered.unmount();
    }

    const empty = renderCell("cycle", frozen, issue({ cycle_id: null }));
    expect(within(empty.container).getByText("None")).toBeInTheDocument();
    empty.unmount();

    for (const key of ["created_on", "updated_on", "link", "attachment_count", "sub_issue_count"]) {
      const rendered = renderCell(key, frozen, issue());
      expect(rendered.container.querySelector("[data-readonly-property]")).not.toBeNull();
      expect(within(rendered.container).queryByRole("button")).not.toBeInTheDocument();
      rendered.unmount();
    }
  });

  it("renders a frozen column it does not know as a None value", () => {
    // The sheet's own column list never reaches this arm; it is the
    // defensive default for a column added ahead of its renderer (say a
    // new count), so a frozen row shows None instead of an edit trigger.
    const frozen = context({ isRowReadOnly: () => true });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const unknown = render(
      <QueryClientProvider client={queryClient}>
        <table>
          <tbody>
            <tr>
              <SheetPropertyCell
                issue={issue()}
                column={{ key: "future_count", label: "Future", sort: { ascending: "x", descending: "-x" } }}
                context={frozen}
                rowIndex={0}
                columnIndex={1}
                onKeyDown={() => {}}
              />
            </tr>
          </tbody>
        </table>
      </QueryClientProvider>
    );
    expect(unknown.container.querySelector('[data-readonly-property="parent"]')).not.toBeNull();
    expect(within(unknown.container).getByText("None")).toBeInTheDocument();
    expect(within(unknown.container).queryByRole("button")).not.toBeInTheDocument();
  });

  it("renders frozen pickers as None until their reference data loads", async () => {
    // Nothing answers these reads: the row still paints values rather than
    // suspending or showing a trigger, which is the contract for a frozen
    // row whose references have not arrived yet.
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const frozen = context({ isRowReadOnly: () => true });
    const row = issue({ state_id: null, assignee_ids: [MEMBER], label_ids: [LABEL], module_ids: [MODULE] });
    for (const key of ["state", "assignee", "labels", "modules"]) {
      const rendered = renderCell(key, frozen, row);
      await vi.waitFor(() => expect(rendered.container.querySelector("[data-readonly-property]"), key).not.toBeNull());
      expect(within(rendered.container).queryByRole("button"), key).not.toBeInTheDocument();
      rendered.unmount();
    }
  });

  it("names frozen references once their pickers answer", async () => {
    stubClient();
    const frozen = context({ isRowReadOnly: () => true });
    const row = issue({ cycle_id: CYCLE, state_id: STATE, label_ids: [LABEL], module_ids: [MODULE] });
    const state = renderCell("state", frozen, row);
    await vi.waitFor(() => expect(within(state.container).getByText("In progress")).toBeInTheDocument());
    state.unmount();
    const cycle = renderCell("cycle", frozen, row);
    await vi.waitFor(() => expect(within(cycle.container).getByText("Cycle 1")).toBeInTheDocument());
    cycle.unmount();
    const modules = renderCell("modules", frozen, row);
    await vi.waitFor(() => expect(within(modules.container).getByText("1 module")).toBeInTheDocument());
    modules.unmount();
  });

  it("falls back to the raw id while frozen references are still resolving", () => {
    // The pickers have not answered: the row shows the stored ids rather
    // than an empty value and no trigger.
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const frozen = context({ isRowReadOnly: () => true });
    const row = issue({ assignee_ids: [MEMBER], label_ids: [LABEL], module_ids: [MODULE] });
    const assignee = renderCell("assignee", frozen, row);
    expect(within(assignee.container).getAllByText(MEMBER.slice(0, 2)).length).toBeGreaterThan(0);
    assignee.unmount();
    const labels = renderCell("labels", frozen, row);
    expect(within(labels.container).getByText("1 label")).toBeInTheDocument();
    labels.unmount();
    const modules = renderCell("modules", frozen, row);
    expect(within(modules.container).getByText("1 module")).toBeInTheDocument();
    modules.unmount();
  });

  it("names frozen references and ignores ids the reference data lacks", async () => {
    stubClient();
    const frozen = context({ isRowReadOnly: () => true });
    const unknown = "923e4567-e89b-12d3-a456-426614174008";
    const assignees = renderCell("assignee", frozen, issue({ assignee_ids: [MEMBER, unknown] }));
    await vi.waitFor(() => expect(within(assignees.container).getByText(/Pat Member/)).toBeInTheDocument());
    assignees.unmount();
    const labels = renderCell("labels", frozen, issue({ label_ids: [LABEL, unknown] }));
    expect(within(labels.container).getByText("2 labels")).toBeInTheDocument();
    labels.unmount();
    const modules = renderCell("modules", frozen, issue({ module_ids: [MODULE, unknown] }));
    expect(within(modules.container).getByText("2 modules")).toBeInTheDocument();
    modules.unmount();
    const state = renderCell("state", frozen, issue({ state_id: unknown }));
    expect(within(state.container).getByText("None")).toBeInTheDocument();
  });

  it("shows None for frozen multi-value columns with nothing assigned", () => {
    stubClient();
    const frozen = context({ isRowReadOnly: () => true });
    for (const key of ["labels", "modules"]) {
      const rendered = renderCell(key, frozen, issue({ label_ids: [], module_ids: [] }));
      expect(within(rendered.container).getByText("None"), key).toBeInTheDocument();
      rendered.unmount();
    }
  });

  it("resolves frozen label and module names once their pickers answer", async () => {
    stubClient();
    const frozen = context({ isRowReadOnly: () => true });
    const unknown = "923e4567-e89b-12d3-a456-426614174008";
    const labels = renderCell("labels", frozen, issue({ label_ids: [LABEL, unknown] }));
    await vi.waitFor(() => expect(within(labels.container).getByText("2 labels")).toBeInTheDocument());
    labels.unmount();
    const modules = renderCell("modules", frozen, issue({ module_ids: [MODULE, unknown] }));
    await vi.waitFor(() => expect(within(modules.container).getByText("2 modules")).toBeInTheDocument());
    modules.unmount();
    const named = renderCell("state", frozen, issue({ state_id: STATE }));
    await vi.waitFor(() => expect(within(named.container).getByText("In progress")).toBeInTheDocument());
    named.unmount();
  });

  it("falls back to the stored estimate when no option matches", async () => {
    stubClient();
    const frozen = context({ isRowReadOnly: () => true });
    const known = renderCell("estimate", frozen, issue({ estimate_point: ESTIMATE }));
    await vi.waitFor(() => expect(within(known.container).queryByText(ESTIMATE)).not.toBeInTheDocument());
    known.unmount();
    const unknown = renderCell("estimate", frozen, issue({ estimate_point: "no-such-option" }));
    expect(within(unknown.container).getByText("no-such-option")).toBeInTheDocument();
    unknown.unmount();
    const clear = renderCell("estimate", frozen, issue({ estimate_point: null }));
    expect(within(clear.container).getByText("None")).toBeInTheDocument();
    clear.unmount();
    const blank = renderCell("estimate", frozen, issue({ estimate_point: undefined }));
    expect(within(blank.container).getByText("None")).toBeInTheDocument();
  });

  it("names a value that belongs to a superseded system read-only (EST-022)", async () => {
    stubClient();
    const superseded = [
      {
        id: ESTIMATE,
        workspace: PROJECT,
        project: PROJECT,
        name: "Sizes",
        description: "",
        type: "categories",
        last_used: false,
        points: [{ id: "no-such-option", key: 1, value: "Large", description: "" }],
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ];
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(workspaceScope(SLUG).concat("pickers", "estimates", PROJECT), superseded);
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <table>
          <tbody>
            <tr>
              <SheetPropertyCell
                issue={issue({ estimate_point: "no-such-option" })}
                column={column("estimate")}
                context={context({ isRowReadOnly: () => true })}
                rowIndex={0}
                columnIndex={1}
                onKeyDown={() => {}}
              />
            </tr>
          </tbody>
        </table>
      </QueryClientProvider>
    );
    expect(within(container).getByText("Large")).toBeInTheDocument();
  });

  it("renders frozen dates and an unknown column as None", () => {
    const frozen = context({ isRowReadOnly: () => true });
    const start = renderCell("start_date", frozen, issue({ start_date: "2026-10-15" }));
    expect(within(start.container).getByText("2026-10-15")).toBeInTheDocument();
    start.unmount();
    const noStart = renderCell("start_date", frozen, issue({ start_date: null, target_date: "2026-10-20" }));
    expect(within(noStart.container).getByText("None")).toBeInTheDocument();
    noStart.unmount();
    const due = renderCell("due_date", frozen, issue({ target_date: "2026-10-20" }));
    expect(within(due.container).getByText("2026-10-20")).toBeInTheDocument();
    due.unmount();
    const noDue = renderCell("due_date", frozen, issue({ target_date: null }));
    expect(within(noDue.container).getByText("None")).toBeInTheDocument();
  });

  it("renders frozen counts without a trigger", () => {
    const frozen = context({ isRowReadOnly: () => true });
    const link = renderCell("link", frozen, issue());
    expect(within(link.container).getByText("1")).toBeInTheDocument();
    expect(within(link.container).queryByRole("button")).not.toBeInTheDocument();
    link.unmount();
    const attachment = renderCell("attachment_count", frozen, issue());
    expect(within(attachment.container).getByText("3")).toBeInTheDocument();
    attachment.unmount();
    const subIssues = renderCell("sub_issue_count", frozen, issue());
    expect(within(subIssues.container).getByText("2 sub-work items")).toBeInTheDocument();
    subIssues.unmount();
    const blank = issue({ link_count: null, attachment_count: null, sub_issues_count: null });
    for (const [key, text] of [
      ["link", "0"],
      ["attachment_count", "0"],
      ["sub_issue_count", "0 sub-work items"],
    ] as const) {
      const empty = renderCell(key, frozen, blank);
      expect(within(empty.container).getByText(text), key).toBeInTheDocument();
      empty.unmount();
    }
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

  it("renders read-only cycle, start date, due date and estimate values", async () => {
    stubClient();
    const { unmount } = renderCell("cycle", context({ editable: false }), issue({ cycle_id: CYCLE }));
    expect(screen.getByText(CYCLE.slice(0, 8))).toBeInTheDocument();
    unmount();
    const start = renderCell("start_date", context({ editable: false }), issue({ start_date: "2026-10-15" }));
    expect(screen.getByText("Oct 15, 2026")).toBeInTheDocument();
    start.unmount();
    const due = renderCell("due_date", context({ editable: false }), issue({ target_date: "2026-10-20" }));
    expect(screen.getByText("Oct 20, 2026")).toBeInTheDocument();
    due.unmount();
    const estimate = renderCell("estimate", context({ editable: false }), issue({ estimate_point: 3 }));
    expect(screen.getByText("3")).toBeInTheDocument();
    estimate.unmount();
    const editableEstimate = renderCell("estimate", context(), issue({ estimate_point: 3 }));
    fireEvent.click(within(editableEstimate.container).getByRole("button"));
    await screen.findByRole("option", { name: "3" });
    editableEstimate.unmount();
    const cycle = renderCell("cycle", context({ editable: false }), issue({ cycle_id: null }));
    expect(cycle.container.querySelector("td")?.textContent).toBe("");
    const unknown = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <table>
          <tbody>
            <tr>
              <SheetPropertyCell
                issue={issue()}
                column={{ key: "key", label: "ID", sort: { ascending: "key", descending: "-key" } }}
                context={context({ editable: false })}
                rowIndex={0}
                columnIndex={1}
                onKeyDown={() => {}}
              />
            </tr>
          </tbody>
        </table>
      </QueryClientProvider>
    );
    expect(unknown.container.querySelector("td")?.textContent).toBe("");
  });

  it("writes assignees, labels and estimate through the picker and refocuses", async () => {
    stubClient();
    const ctx = context();
    const assignee = renderCell("assignee", ctx);
    fireEvent.click(screen.getByRole("button", { name: "Assignees" }));
    await screen.findByRole("option", { name: /Pat Member/ });
    fireEvent.click(screen.getByRole("option", { name: /Pat Member/ }));
    await vi.waitFor(() => expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { assignee_ids: [MEMBER] }));
    await vi.waitFor(() => expect(ctx.focusCell).toHaveBeenCalledWith(ISSUE, "assignee", 1));
    assignee.unmount();

    const estimate = renderCell("estimate", ctx);
    fireEvent.click(within(estimate.container).getByRole("button"));
    await screen.findByRole("option", { name: /^3$/ });
    fireEvent.click(screen.getByRole("option", { name: /^3$/ }));
    await vi.waitFor(() => expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { estimate_point: expect.any(String) }));
    estimate.unmount();

    const labels = renderCell("labels", ctx, issue({ label_ids: [LABEL] }));
    fireEvent.click(screen.getByTestId("label-picker-trigger"));
    await screen.findByTestId("label-picker-option");
    fireEvent.click(screen.getByTestId("label-picker-option"));
    await vi.waitFor(() => expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { label_ids: [] }));
    expect(within(labels.container).getByRole("button", { name: /Labels for Fix login: Bug/ })).toBeInTheDocument();
  });

  it("writes a due date while the start date is unset", async () => {
    stubClient();
    const ctx = context();
    renderCell("due_date", ctx, issue({ start_date: undefined, target_date: null }));
    fireEvent.click(screen.getByTestId(`sheet-due-${ISSUE}-trigger`));
    const calendar = screen.getByTestId(`sheet-due-${ISSUE}-calendar`);
    fireEvent.click(within(calendar).getByRole("button", { name: "October 15, 2026" }));
    expect(ctx.onUpdate).toHaveBeenCalledWith(ISSUE, { start_date: null, target_date: "2026-10-15" });
  });

  it("disables the cycle picker on a completed cycle row", async () => {
    stubClient("started", "COMPLETED");
    renderCell("cycle", context(), issue({ cycle_id: CYCLE }));
    await vi.waitFor(() => expect(screen.getByRole("button", { name: "No cycle" })).toBeDisabled());
  });

  it("renders a zero count when the sub-issue count is absent", () => {
    const ctx = context();
    renderCell("sub_issue_count", ctx, issue({ sub_issues_count: null }));
    const empty = screen.getByRole("button", { name: "0 sub-work items" });
    fireEvent.click(empty);
    expect(ctx.onOpenSubIssues).not.toHaveBeenCalled();
  });

  it("renders an empty state name before the state list arrives", () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      return new Promise<never>(() => undefined);
    }) as never);
    renderCell("state", context({ editable: false }), issue({ state_id: STATE }));
    expect(screen.getByLabelText("State for Fix login")).toHaveTextContent("");
  });

  it("renders an empty read-only state name when the state list has no match", async () => {
    stubClient();
    const ctx = context({ editable: false });
    const rendered = renderCell("state", ctx, issue({ state_id: STATE }));
    await vi.waitFor(() =>
      expect(rendered.queryClient.getQueryData(["ws", SLUG, "pickers", "states", PROJECT])).toBeDefined()
    );
    rendered.unmount();
    const unresolved = renderCell(
      "state",
      context({ editable: false }),
      issue({ state_id: "923e4567-e89b-12d3-a456-426614174008" })
    );
    await vi.waitFor(() =>
      expect(unresolved.queryClient.getQueryData(["ws", SLUG, "pickers", "states", PROJECT])).toBeDefined()
    );
    expect(screen.getByLabelText("State for Fix login")).toHaveTextContent("");
  });

  it("still refocuses when a cell write is refused", async () => {
    stubClient();
    const ctx = context({ onUpdate: vi.fn(async () => Promise.reject(new Error("refused"))) });
    renderCell("priority", ctx);
    fireEvent.click(screen.getByRole("button", { name: /high/i }));
    fireEvent.click(screen.getByRole("option", { name: "Urgent" }));
    await vi.waitFor(() => expect(ctx.focusCell).toHaveBeenCalledWith(ISSUE, "priority", 1));
  });
});
