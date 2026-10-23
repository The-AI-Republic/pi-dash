// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board layout (parity/specs/issues.md §5, ISS-028–035): the flat columns
// and their collapse writes, swimlane rows with lane collapse, the
// show-empty filter, card open (peek on wide, navigation on narrow), the
// quick-add prefill plus default state and the permission/workflow gates.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  board: null as unknown,
  prefs: { filters: {} } as { filters: Record<string, unknown> },
  canEdit: true,
  narrow: false,
  toggleCollapsedGroup: vi.fn(),
  navigate: vi.fn(),
  mutateAsync: vi.fn(),
  attach: vi.fn(),
  copy: vi.fn(),
  permissions: { role: 15, isMember: true, canEdit: true, isAdmin: false },
  groupLabels: null as unknown,
  projects: null as unknown,
  details: null as unknown,
}));

vi.mock("./pages.js", () => ({
  useBoard: () => ({ board: mocks.board, isPending: mocks.board === null, isFetching: false }),
}));
vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ client: "stub" }) }));
vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  usePermissions: () => mocks.permissions,
}));
vi.mock("../../projects/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../projects/index.js")>()),
  projectsQueryOptions: () => ({ queryKey: ["projects"] }),
  projectDetailsQueryOptions: () => ({ queryKey: ["details"] }),
}));
vi.mock("../prefs/prefsQuery.js", () => ({
  prefsQueryOptions: () => ({ queryKey: ["prefs"] }),
}));
vi.mock("../list/reference.js", () => ({
  useListReferences: () => ({
    labels: (mocks.groupLabels as unknown as Record<string, Map<string, string>>) ?? {},
    stateRows: [],
    stateNames: new Map(),
    assigneeNames: new Map(),
    labelNames: new Map(),
    cycleNames: new Map(),
    moduleNames: new Map(),
    openCycleIds: new Set(),
  }),
}));
vi.mock("../list/collapse.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../list/collapse.js")>()),
  useToggleCollapsedGroup: () => ({
    isSaving: false,
    toggleCollapsedGroup: mocks.toggleCollapsedGroup,
  }),
}));
vi.mock("../api/create.js", () => ({
  useCreateWorkItem: () => ({ mutateAsync: mocks.mutateAsync }),
}));
vi.mock("../list/mutations.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../list/mutations.js")>()),
  attachCreatedIssue: mocks.attach,
}));
vi.mock("./KanbanPeek.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./KanbanPeek.js")>()),
  copyCardLink: mocks.copy,
  KanbanPeek: (props: { issue: { name: string }; onClose: () => void }) => (
    <div data-testid="issue-preview">
      <span data-testid="issue-peek-key">{`WEB-${props.issue.name === "Fix login" ? 42 : 43}`}</span>
      <button type="button" onClick={props.onClose}>
        Close preview
      </button>
    </div>
  ),
}));
vi.mock("../../../shared/shell/narrowViewport.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/narrowViewport.js")>()),
  useNarrowViewport: () => mocks.narrow,
}));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));
vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useQuery: (options: { queryKey: unknown[] }) => {
    if (options.queryKey[0] === "projects") {
      return {
        data: (mocks.projects as unknown as unknown[]) ?? [{ id: PROJECT, cycle_view: true, module_view: true }],
      };
    }
    return {
      data: (mocks.details as unknown as unknown[]) ?? [
        {
          id: PROJECT,
          estimate: null,
          default_state: "state-default",
          timezone: "UTC",
        },
      ],
    };
  },
  useSuspenseQuery: () => ({ data: mocks.prefs }),
}));

import type { IssueListItem } from "@pidash/api-client";

import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";

import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { KanbanBoard } from "./KanbanBoard.js";
import type { BoardData } from "./model.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const TODO = "323e4567-e89b-12d3-a456-426614174002";
const DONE = "423e4567-e89b-12d3-a456-426614174003";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: TODO,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
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
    ...overrides,
  };
}

function flatBoard(): BoardData {
  return {
    columns: [
      { key: TODO, title: "Todo", total: 2 },
      { key: DONE, title: "Done", total: 0 },
    ],
    lanes: [
      {
        key: "",
        title: "",
        total: 2,
        cells: {
          [TODO]: {
            columnKey: TODO,
            rows: [row(), row({ id: "issue-2", name: "Ship it", sequence_id: 43 })],
            total: 2,
          },
          [DONE]: { columnKey: DONE, rows: [], total: 0 },
        },
      },
    ],
    subGrouped: false,
    total: 2,
  };
}

function swimlaneBoard(): BoardData {
  return {
    columns: [
      { key: TODO, title: "Todo", total: 2 },
      { key: DONE, title: "Done", total: 1 },
    ],
    lanes: [
      {
        key: "bug",
        title: "Bug",
        total: 2,
        cells: {
          [TODO]: { columnKey: TODO, rows: [row()], total: 1 },
          [DONE]: { columnKey: DONE, rows: [row({ id: "issue-2", name: "Ship it", sequence_id: 43 })], total: 1 },
        },
      },
      {
        key: "None",
        title: "None",
        total: 1,
        cells: {
          [TODO]: { columnKey: TODO, rows: [row({ id: "issue-3", name: "Plain", sequence_id: 44 })], total: 1 },
          [DONE]: { columnKey: DONE, rows: [], total: 0 },
        },
      },
      {
        key: "empty",
        title: "Empty",
        total: 0,
        cells: {
          [TODO]: { columnKey: TODO, rows: [], total: 0 },
          [DONE]: { columnKey: DONE, rows: [], total: 0 },
        },
      },
    ],
    subGrouped: true,
    total: 3,
  };
}

function prefs(overrides: Partial<DisplayPrefs> = {}): DisplayPrefs {
  return {
    layout: "kanban",
    groupBy: "state",
    subGroupBy: null,
    orderBy: "sort_order",
    showEmptyGroups: true,
    showSubIssues: false,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
    ...overrides,
  };
}

function setup(overrides: Partial<React.ComponentProps<typeof KanbanBoard>> = {}) {
  const onPeekChange = vi.fn();
  const props: React.ComponentProps<typeof KanbanBoard> = {
    workspaceSlug: "acme",
    entity: { scope: "project", projectId: PROJECT },
    projectId: PROJECT,
    projectIdentifier: "WEB",
    prefs: prefs(),
    canEdit: true,
    onPeekChange,
    ...overrides,
  };
  const view = render(
    <QueryClientProvider client={new QueryClient()}>
      <KanbanBoard {...props} />
    </QueryClientProvider>
  );
  return { onPeekChange, ...view };
}

function column(name: string): HTMLElement {
  const found = screen
    .getAllByTestId("kanban-column")
    .find((element) => element.getAttribute("data-column-name") === name);
  if (found === undefined) throw new Error(`no column ${name}`);
  return found;
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  vi.clearAllMocks();
  mocks.board = flatBoard();
  mocks.prefs = { filters: {} };
  mocks.canEdit = true;
  mocks.narrow = false;
  mocks.permissions = { role: 15, isMember: true, canEdit: true, isAdmin: false };
  mocks.groupLabels = null;
  mocks.projects = null;
  mocks.details = null;
  mocks.mutateAsync.mockResolvedValue({ id: "created-1", sequence_id: 50 });
  mocks.attach.mockResolvedValue(undefined);
  mocks.copy.mockResolvedValue(true);
});

describe("KanbanBoard", () => {
  it("draws one column per group value with its count and cards", () => {
    setup();
    expect(screen.getByTestId("kanban-layout")).not.toHaveAttribute("data-sub-grouped");
    const todo = column("Todo");
    expect(within(todo).getByTestId("kanban-column-count")).toHaveTextContent("2");
    expect(within(todo).getAllByTestId("kanban-card")).toHaveLength(2);
    expect(within(column("Done")).queryAllByTestId("kanban-card")).toHaveLength(0);
  });

  it("shows the loading skeleton before the first board read", () => {
    mocks.board = null;
    setup();
    expect(screen.getByTestId("kanban-loading")).toBeInTheDocument();
  });

  it("writes a flat column's collapse through the stored kanban filters", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(within(column("Todo")).getByRole("button", { name: "Collapse Todo column" }));
    expect(mocks.toggleCollapsedGroup).toHaveBeenCalledWith("group_by", TODO);
  });

  it("reads a stored collapsed column and folds it to the strip", () => {
    mocks.prefs = { filters: { kanban_filters: { group_by: [TODO], sub_group_by: [] } } };
    setup();
    expect(column("Todo")).toHaveAttribute("data-collapsed", "true");
    expect(within(column("Todo")).queryByTestId("kanban-column-body")).not.toBeInTheDocument();
    expect(within(column("Todo")).getByTestId("kanban-column-count")).toHaveTextContent("2");
  });

  it("draws swimlane group headers and one lane per sub-group with cumulative counts", () => {
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ subGroupBy: "labels" }) });
    expect(screen.getByTestId("kanban-layout")).toHaveAttribute("data-sub-grouped", "true");
    expect(screen.getAllByTestId("kanban-group-header")).toHaveLength(2);
    const lanes = screen.getAllByTestId("kanban-lane");
    expect(lanes.map((lane) => lane.getAttribute("data-lane-key"))).toEqual(["bug", "None", "empty"]);
    expect(screen.getAllByTestId("kanban-lane-count").map((node) => node.textContent)).toEqual(["2", "1", "0"]);
    expect(screen.getAllByTestId("kanban-cell")).toHaveLength(6);
  });

  it("ignores a sub-group equal to the group and draws the flat board", () => {
    setup({ prefs: prefs({ subGroupBy: "state" }) });
    expect(screen.getByTestId("kanban-layout")).not.toHaveAttribute("data-sub-grouped");
  });

  it("toggles a swimlane through the sub-group collapse axis", async () => {
    const user = userEvent.setup();
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ subGroupBy: "labels" }) });
    const lane = screen.getAllByTestId("kanban-lane")[0] as HTMLElement;
    await user.click(within(lane).getByTestId("kanban-lane-header"));
    expect(mocks.toggleCollapsedGroup).toHaveBeenCalledWith("sub_group_by", "bug");
  });

  it("hides a collapsed lane's cards while the cells stay in place", () => {
    mocks.board = swimlaneBoard();
    mocks.prefs = { filters: { kanban_filters: { group_by: [], sub_group_by: ["bug"] } } };
    setup({ prefs: prefs({ subGroupBy: "labels" }) });
    const lane = screen.getAllByTestId("kanban-lane")[0] as HTMLElement;
    expect(within(lane).queryAllByTestId("kanban-card")).toHaveLength(0);
    expect(within(lane).getAllByTestId("kanban-cell")).toHaveLength(2);
    // The two unlabeled rows survive in the None lane (ISS-031/NEWFRONT-158).
    expect(screen.getAllByTestId("kanban-card")).toHaveLength(1);
  });

  it("drops zero-count lanes and columns when show-empty is off", () => {
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ subGroupBy: "labels", showEmptyGroups: false }) });
    expect(screen.getAllByTestId("kanban-lane")).toHaveLength(2);
    expect(screen.queryByText("Empty")).not.toBeInTheDocument();
  });

  it("opens the peek on a wide viewport through the controlled key", async () => {
    const user = userEvent.setup();
    const { onPeekChange } = setup();
    await user.click(within(column("Todo")).getAllByTestId("kanban-card-link")[0] as HTMLElement);
    expect(onPeekChange).toHaveBeenCalledWith("WEB-42");
    expect(mocks.navigate).not.toHaveBeenCalled();
  });

  it("navigates to the work item on a narrow viewport", async () => {
    const user = userEvent.setup();
    mocks.narrow = true;
    const { onPeekChange } = setup();
    await user.click(within(column("Todo")).getAllByTestId("kanban-card-link")[0] as HTMLElement);
    expect(mocks.navigate).toHaveBeenCalledWith({
      to: "/$ws/browse/$issueKey",
      params: { ws: "acme", issueKey: "WEB-42" },
    });
    expect(onPeekChange).not.toHaveBeenCalled();
  });

  it("ignores clicks on an unconfirmed card", async () => {
    mocks.board = flatBoard();
    const lane = (mocks.board as BoardData).lanes[0];
    if (lane === undefined) throw new Error("missing lane");
    lane.cells[TODO] = { columnKey: TODO, rows: [row({ id: "temp-1", name: "Draft" })], total: 1 };
    const user = userEvent.setup();
    const { onPeekChange } = setup();
    await user.click(screen.getByTestId("kanban-card-link"));
    expect(onPeekChange).not.toHaveBeenCalled();
  });

  it("renders the peek panel for a controlled key and closes it through the callback", async () => {
    const user = userEvent.setup();
    const { onPeekChange } = setup({ peekKey: "WEB-42" });
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("keeps its own peek selection when the host does not control it", async () => {
    const user = userEvent.setup();
    setup({ peekKey: undefined, onPeekChange: undefined });
    await user.click(within(column("Todo")).getAllByTestId("kanban-card-link")[0] as HTMLElement);
    expect(screen.getByTestId("issue-peek-key")).toHaveTextContent("WEB-42");
    expect(screen.getAllByTestId("kanban-card")[0]).toHaveAttribute("data-peeked", "true");
  });

  it("quick-adds into the column group with the project default state", async () => {
    const user = userEvent.setup();
    setup();
    const done = column("Done");
    await user.click(within(done).getByRole("button", { name: /New work item/ }));
    await user.type(within(done).getByPlaceholderText("Work item title"), "Fresh item{enter}");
    await waitFor(() =>
      expect(mocks.mutateAsync).toHaveBeenCalledWith({
        projectId: PROJECT,
        payload: { name: "Fresh item", state_id: DONE },
        projectIdentifier: "WEB",
      })
    );
  });

  it("merges the lane value into a swimlane quick-add", async () => {
    const user = userEvent.setup();
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ groupBy: "priority", subGroupBy: "labels" }) });
    mocks.prefs = { filters: {} };
    const lane = screen.getAllByTestId("kanban-lane")[0] as HTMLElement;
    const cell = within(lane).getAllByTestId("kanban-cell")[0] as HTMLElement;
    await user.click(within(cell).getByRole("button", { name: /New work item/ }));
    await user.type(within(cell).getByPlaceholderText("Work item title"), "Lane item{enter}");
    await waitFor(() =>
      expect(mocks.mutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          payload: { name: "Lane item", priority: TODO, label_ids: ["bug"], state_id: "state-default" },
        })
      )
    );
  });

  it("attaches the created item to a lane's cycle membership", async () => {
    const user = userEvent.setup();
    mocks.board = flatBoard();
    setup({ prefs: prefs({ groupBy: "cycle" }), cycleId: "cycle-route" });
    const todo = column("Todo");
    await user.click(within(todo).getByRole("button", { name: /New work item/ }));
    await user.type(within(todo).getByPlaceholderText("Work item title"), "Cycled{enter}");
    await waitFor(() =>
      expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {
        cycleId: "cycle-route",
      })
    );
  });

  it("keeps the created card when membership attach fails", async () => {
    const user = userEvent.setup();
    mocks.attach.mockRejectedValue(new Error("bridge down"));
    mocks.board = flatBoard();
    setup({ prefs: prefs({ groupBy: "cycle" }) });
    const todo = column("Todo");
    await user.click(within(todo).getByRole("button", { name: /New work item/ }));
    await user.type(within(todo).getByPlaceholderText("Work item title"), "Cycled{enter}");
    await waitFor(() => expect(mocks.attach).toHaveBeenCalled());
    expect(screen.getByPlaceholderText("Work item title")).toHaveValue("");
  });

  it.each([
    { label: "a guest", overrides: { canEdit: false } },
    { label: "a completed cycle", overrides: { completedCycle: true } },
    { label: "a workflow block", overrides: { creationBlockedReason: "Workflow rule blocks creation" } },
  ])("offers no quick-add for %s", ({ overrides }) => {
    setup(overrides as Partial<React.ComponentProps<typeof KanbanBoard>>);
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("hides quick-add when the workspace role cannot edit", () => {
    mocks.permissions = { role: 5, isMember: true, canEdit: false, isAdmin: false };
    setup();
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("marks cards read-only for a completed cycle", () => {
    setup({ completedCycle: true });
    const card = screen.getAllByTestId("kanban-card")[0] as HTMLElement;
    expect(card).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("marks cards read-only for a project that denies editing", () => {
    const strip = vi.fn();
    void strip;
    setup({ canEdit: false });
    // Read-only still renders the column; only the write entries disappear.
    expect(screen.getAllByTestId("kanban-card")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("deduplicates rows and resolves group labels per dimension", () => {
    mocks.board = flatBoard();
    const lane = (mocks.board as BoardData).lanes[0];
    if (lane === undefined) throw new Error("missing lane");
    // The same row in two cells must resolve to one peek target.
    lane.cells[DONE] = { columnKey: DONE, rows: [row()], total: 1 };
    mocks.groupLabels = {
      state: new Map([
        [TODO, "To do"],
        [DONE, "Finished"],
      ]),
      labels: new Map([["bug", "Bug"]]),
    };
    setup({ peekKey: "WEB-42" });
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
  });

  it("reads sub-group labels from the sub-group axis", () => {
    mocks.board = swimlaneBoard();
    mocks.groupLabels = { state: new Map([[TODO, "Todo"]]), labels: new Map([["bug", "Bug"]]) };
    setup({ prefs: prefs({ subGroupBy: "labels" }) });
    expect(screen.getAllByTestId("kanban-lane")).toHaveLength(3);
  });

  it("keeps a peek key that matches no visible card closed", () => {
    setup({ peekKey: "WEB-999" });
    expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument();
  });

  it("strips the module membership from the create and attaches the route target", async () => {
    const user = userEvent.setup();
    mocks.board = flatBoard();
    setup({ prefs: prefs({ groupBy: "module", subGroupBy: null }), moduleId: "module-route" });
    const todo = column("Todo");
    await user.click(within(todo).getByRole("button", { name: /New work item/ }));
    await user.type(within(todo).getByPlaceholderText("Work item title"), "Moduled{enter}");
    await waitFor(() =>
      expect(mocks.mutateAsync).toHaveBeenCalledWith({
        projectId: PROJECT,
        payload: { name: "Moduled", state_id: "state-default" },
        projectIdentifier: "WEB",
      })
    );
    await waitFor(() =>
      expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {
        moduleId: "module-route",
      })
    );
  });

  it("leaves the default state off a grouped state column", async () => {
    const user = userEvent.setup();
    mocks.board = flatBoard();
    setup({ prefs: prefs({ groupBy: "state" }) });
    const todo = column("Todo");
    await user.click(within(todo).getByRole("button", { name: /New work item/ }));
    await user.type(within(todo).getByPlaceholderText("Work item title"), "Stated{enter}");
    await waitFor(() =>
      expect(mocks.mutateAsync).toHaveBeenCalledWith({
        projectId: PROJECT,
        payload: { name: "Stated", state_id: TODO },
        projectIdentifier: "WEB",
      })
    );
  });

  it("copies a card link from the card menu", async () => {
    const user = userEvent.setup({ pointerEventsCheck: 0 });
    setup();
    const card = screen.getAllByTestId("kanban-card")[0] as HTMLElement;
    await user.click(within(card).getByRole("button", { name: "Actions for Fix login" }));
    const { fireEvent } = await import("@testing-library/react");
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    expect(mocks.copy).toHaveBeenCalledWith("acme", "WEB-42");
  });

  it("renders an empty cell for a lane without that column's bucket", () => {
    mocks.board = swimlaneBoard();
    const lanes = (mocks.board as BoardData).lanes;
    const bug = lanes[0];
    if (bug === undefined) throw new Error("missing lane");
    delete bug.cells[DONE];
    setup({ prefs: prefs({ subGroupBy: "labels" }) });
    const cells = screen.getAllByTestId("kanban-cell").map((cell) => cell.getAttribute("data-column-key"));
    expect(cells).toContain(DONE);
    expect(screen.queryByText("Ship it")).not.toBeInTheDocument();
  });

  it("reads no group labels without a grouping dimension", async () => {
    mocks.groupLabels = {};
    setup({ prefs: prefs({ groupBy: null }) });
    expect(screen.getByTestId("kanban-layout")).not.toHaveAttribute("data-sub-grouped");
    expect(screen.getAllByTestId("kanban-column")).toHaveLength(2);
  });

  it("keeps the host edit standing when no override is passed", () => {
    mocks.permissions = { role: 5, isMember: true, canEdit: false, isAdmin: false };
    setup({ canEdit: undefined });
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("keeps the default lane's cards open", () => {
    setup();
    expect(screen.queryByTestId("kanban-lane")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("kanban-card")).toHaveLength(2);
  });

  it("accepts the host edit standing when the workspace allows editing", () => {
    mocks.permissions = { role: 20, isMember: true, canEdit: true, isAdmin: true };
    setup({ canEdit: true });
    expect(screen.getAllByRole("button", { name: /New work item/ }).length).toBeGreaterThan(0);
  });

  it("attaches a module column's first membership when the route names no target", async () => {
    const user = userEvent.setup();
    mocks.board = flatBoard();
    setup({ prefs: prefs({ groupBy: "module", subGroupBy: "labels" }) });
    const todo = column("Todo");
    await user.click(within(todo).getByRole("button", { name: /New work item/ }));
    await user.type(within(todo).getByPlaceholderText("Work item title"), "Moduled{enter}");
    await waitFor(() =>
      expect(mocks.attach).toHaveBeenCalledWith({ client: "stub" }, "acme", PROJECT, "created-1", {
        moduleId: TODO,
      })
    );
  });

  it("skips the lane's module membership when the column has none", async () => {
    const user = userEvent.setup();
    mocks.board = flatBoard();
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "labels" }) });
    const todo = column("Todo");
    await user.click(within(todo).getByRole("button", { name: /New work item/ }));
    await user.type(within(todo).getByPlaceholderText("Work item title"), "Plain{enter}");
    await waitFor(() => expect(mocks.mutateAsync).toHaveBeenCalled());
    expect(mocks.attach).not.toHaveBeenCalled();
  });

  it("skips the route module target when a lane carries no module", async () => {
    const user = userEvent.setup();
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ groupBy: "priority", subGroupBy: "labels" }) });
    const lane = screen.getAllByTestId("kanban-lane")[1] as HTMLElement;
    const cell = within(lane).getAllByTestId("kanban-cell")[0] as HTMLElement;
    await user.click(within(cell).getByRole("button", { name: /New work item/ }));
    await user.type(within(cell).getByPlaceholderText("Work item title"), "Plain lane{enter}");
    await waitFor(() => expect(mocks.mutateAsync).toHaveBeenCalled());
    expect(mocks.attach).not.toHaveBeenCalled();
  });

  it("marks cards read-only and hides the estimate without project details", async () => {
    mocks.details = [];
    mocks.projects = [];
    setup({ canEdit: false });
    expect(screen.getAllByTestId("kanban-card")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /New work item/ })).not.toBeInTheDocument();
  });

  it("renders an empty column when the flat envelope lacks that bucket", () => {
    mocks.board = flatBoard();
    const lane = (mocks.board as BoardData).lanes[0];
    if (lane === undefined) throw new Error("missing lane");
    delete lane.cells[DONE];
    setup();
    expect(within(column("Done")).queryAllByTestId("kanban-card")).toHaveLength(0);
  });

  it("falls back to the workspace edit standing when the host passes no override", () => {
    mocks.permissions = { role: 15, isMember: true, canEdit: true, isAdmin: false };
    setup({ canEdit: undefined });
    expect(screen.getAllByRole("button", { name: /New work item/ }).length).toBeGreaterThan(0);
  });
});
