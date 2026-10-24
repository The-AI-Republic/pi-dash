// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board layout (parity/specs/issues.md §5, ISS-028–043): the flat columns
// and their collapse writes, swimlane rows with lane collapse, the
// show-empty filter, card open (peek on wide, navigation on narrow), the
// quick-add prefill plus default state, the header add control, pointer
// drag-drop with its overlays, per-column pagination and the
// permission/workflow gates.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  board: null as unknown,
  prefs: { filters: {} } as { filters: Record<string, unknown> },
  canEdit: true,
  narrow: false,
  touch: false,
  toggleCollapsedGroup: vi.fn(),
  navigate: vi.fn(),
  mutateAsync: vi.fn(),
  attach: vi.fn(),
  copy: vi.fn(),
  permissions: { role: 15, isMember: true, canEdit: true, isAdmin: false },
  me: { id: "me-1" } as { id: string } | null,
  groupLabels: null as unknown,
  projects: null as unknown,
  details: null as unknown,
  openCycleIds: new Set<string>(),
  moveCard: vi.fn(),
  attachExisting: vi.fn(),
  attachInput: vi.fn(),
  notifyInfo: vi.fn(),
  loadMoreColumn: vi.fn(),
  loadMoreCell: vi.fn(),
  moreColumns: [] as string[],
  moreCells: [] as string[],
  fetchingColumns: [] as string[],
  fetchingCells: [] as string[],
  createDialog: vi.fn(),
  addExisting: vi.fn(),
  deleteDialog: vi.fn(),
  planBoardMove: vi.fn(),
}));

vi.mock("./pages.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./pages.js")>()),
  useBoard: () => ({
    board: mocks.board,
    isPending: mocks.board === null,
    isFetching: false,
    loadMoreColumn: mocks.loadMoreColumn,
    loadMoreCell: mocks.loadMoreCell,
    columnHasMore: (key: string) => mocks.moreColumns.includes(key),
    cellHasMore: (key: string, lane: string) => mocks.moreCells.includes(`${lane}:${key}`),
    columnFetching: (key: string) => mocks.fetchingColumns.includes(key),
    cellFetching: (key: string, lane: string) => mocks.fetchingCells.includes(`${lane}:${key}`),
  }),
}));
vi.mock("./drag.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./drag.js")>()),
  planBoardMove: (...args: unknown[]) => (mocks.planBoardMove as (...call: unknown[]) => unknown)(...args),
}));
vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ client: "stub" }) }));
vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  usePermissions: () => mocks.permissions,
  useMe: () => ({ data: mocks.me }),
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
    openCycleIds: mocks.openCycleIds,
  }),
}));
vi.mock("../calendar/useMedia.js", () => ({
  useTouchInput: () => mocks.touch,
}));
vi.mock("../list/attach.js", () => ({
  useAttachExisting: (input: unknown) => {
    mocks.attachInput(input);
    return { attach: mocks.attachExisting, isAttaching: false };
  },
}));
vi.mock("./mutations.js", () => ({
  useBoardCardMove: () => ({ isMoving: false, moveCard: mocks.moveCard }),
}));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/index.js")>()),
  notifyInfo: (...args: unknown[]) => mocks.notifyInfo(...args),
}));
vi.mock("../components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: {
    open: boolean;
    initialValues: unknown;
    onOpenChange: (open: boolean) => void;
    onCreated: () => void;
  }) => {
    mocks.createDialog(props);
    if (!props.open) return null;
    return (
      <div data-testid="create-dialog" data-initial={JSON.stringify(props.initialValues)}>
        <button type="button" onClick={() => props.onOpenChange(false)}>
          Close create
        </button>
        <button type="button" onClick={() => props.onCreated()}>
          Finish create
        </button>
      </div>
    );
  },
}));
vi.mock("../list/AddExistingModal.js", () => ({
  AddExistingModal: (props: {
    open: boolean;
    excludeIds: string[];
    onOpenChange: (open: boolean) => void;
    onAttach: (ids: string[]) => void;
  }) => {
    mocks.addExisting(props);
    if (!props.open) return null;
    return (
      <div data-testid="add-existing-modal" data-exclude={props.excludeIds.join(",")}>
        <button type="button" onClick={() => props.onOpenChange(false)}>
          Close attach
        </button>
        <button type="button" onClick={() => props.onAttach(["picked-1"])}>
          Attach picked
        </button>
      </div>
    );
  },
}));
vi.mock("../lifecycle/DeleteWorkItemDialog.js", () => ({
  DeleteWorkItemDialog: (props: { open: boolean; onOpenChange: (open: boolean) => void }) => {
    mocks.deleteDialog(props);
    if (!props.open) return null;
    return (
      <div data-testid="delete-dialog">
        <button type="button" onClick={() => props.onOpenChange(false)}>
          Close delete
        </button>
      </div>
    );
  },
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
vi.mock("../peek/ResolvingIssuePeek.js", () => ({
  ResolvingIssuePeek: (props: { peeked: string; onClose: () => void }) => (
    <div data-testid="issue-preview">
      <span data-testid="issue-peek-key">{props.peeked}</span>
      <button type="button" onClick={props.onClose}>
        Close preview
      </button>
    </div>
  ),
}));
vi.mock("../peek/peekLink.js", () => ({
  copyCardLink: mocks.copy,
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
import { DROPPED_HIGHLIGHT_MS, KanbanBoard } from "./KanbanBoard.js";
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

beforeEach(async () => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  vi.clearAllMocks();
  mocks.board = flatBoard();
  mocks.prefs = { filters: {} };
  mocks.canEdit = true;
  mocks.narrow = false;
  mocks.touch = false;
  mocks.permissions = { role: 15, isMember: true, canEdit: true, isAdmin: false };
  mocks.me = { id: "me-1" };
  mocks.groupLabels = null;
  mocks.projects = null;
  mocks.details = null;
  mocks.openCycleIds = new Set<string>();
  mocks.moreColumns = [];
  mocks.moreCells = [];
  mocks.fetchingColumns = [];
  mocks.fetchingCells = [];
  mocks.mutateAsync.mockResolvedValue({ id: "created-1", sequence_id: 50 });
  mocks.attach.mockResolvedValue(undefined);
  mocks.copy.mockResolvedValue(true);
  mocks.attachExisting.mockResolvedValue(true);
  const real = await vi.importActual<typeof import("./drag.js")>("./drag.js");
  mocks.planBoardMove.mockImplementation((input: never) => real.planBoardMove(input));
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
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

  it("opens the peek on a wide viewport with the work-item id", async () => {
    const user = userEvent.setup();
    const { onPeekChange } = setup();
    await user.click(within(column("Todo")).getAllByTestId("kanban-card-link")[0] as HTMLElement);
    expect(onPeekChange).toHaveBeenCalledWith(ISSUE);
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

  it("renders the peek panel for a controlled id and closes it through the callback", async () => {
    const user = userEvent.setup();
    const { onPeekChange } = setup({ peekKey: ISSUE });
    expect(await screen.findByTestId("issue-preview")).toBeInTheDocument();
    expect(screen.getByTestId("issue-peek-key")).toHaveTextContent(ISSUE);
    expect(screen.getAllByTestId("kanban-card")[0]).toHaveAttribute("data-peeked", "true");
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("lifts the card for a deep-linked key while accepting ids on open", () => {
    setup({ peekKey: "WEB-42" });
    expect(screen.getAllByTestId("kanban-card")[0]).toHaveAttribute("data-peeked", "true");
  });

  it("keeps its own peek selection when the host does not control it", async () => {
    const user = userEvent.setup();
    setup({ peekKey: undefined, onPeekChange: undefined });
    await user.click(within(column("Todo")).getAllByTestId("kanban-card-link")[0] as HTMLElement);
    expect(await screen.findByTestId("issue-peek-key")).toHaveTextContent(ISSUE);
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

  it("mounts the resolving peek for a key that matches no visible card", async () => {
    setup({ peekKey: "WEB-999" });
    expect(await screen.findByTestId("issue-preview")).toBeInTheDocument();
    expect(screen.getByTestId("issue-peek-key")).toHaveTextContent("WEB-999");
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

function pointer(type: string, clientX: number, clientY: number): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, clientX, clientY });
}

function stubRect(element: Element, left: number, top: number, right: number, bottom: number): void {
  element.getBoundingClientRect = () =>
    ({ left, top, right, bottom, width: right - left, height: bottom - top, x: left, y: top }) as DOMRect;
}

function cardByName(name: string): HTMLElement {
  const found = screen
    .getAllByTestId("kanban-card")
    .find((card) => within(card).queryByTestId("kanban-card-name")?.textContent === name);
  if (found === undefined) throw new Error(`no card ${name}`);
  return found;
}

/** Flat-board geometry: Todo left with two stacked cards, Done right, zone top center. */
function stubFlatGeometry(): void {
  stubRect(column("Todo"), 0, 0, 350, 600);
  stubRect(column("Done"), 350, 0, 700, 600);
  const cards = within(column("Todo")).getAllByTestId("kanban-card");
  stubRect(cards[0] as HTMLElement, 8, 60, 342, 160);
  stubRect(cards[1] as HTMLElement, 8, 168, 342, 268);
  stubRect(screen.getByTestId("kanban-delete-zone"), 300, 0, 500, 40);
}

/** Press a card and travel past the threshold so the board's drag starts. */
function pressCard(card: HTMLElement): void {
  fireEvent(card, pointer("pointerdown", 20, 80));
  fireEvent(document, pointer("pointermove", 90, 90));
}

describe("KanbanBoard header create", () => {
  it("opens the create dialog prefilled with the column payload", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-initial", JSON.stringify({ stateId: TODO }));
    const props = mocks.createDialog.mock.calls[0]?.[0] as {
      initialProjectId: string;
      projectIdentifier: string;
      projects: Array<{ id: string }>;
    };
    expect(props.initialProjectId).toBe(PROJECT);
    expect(props.projectIdentifier).toBe("WEB");
    expect(props.projects).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Close create" }));
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
  });

  it("closes the create dialog once the item lands", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await screen.findByTestId("create-dialog");
    await user.click(screen.getByRole("button", { name: "Finish create" }));
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
  });

  it("opens creation without projects when the project row is missing", async () => {
    const user = userEvent.setup();
    mocks.projects = [];
    setup();
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await screen.findByTestId("create-dialog");
    const props = mocks.createDialog.mock.calls[0]?.[0] as { projects: unknown[] };
    expect(props.projects).toEqual([]);
  });

  it("offers create and add-existing in cycle context", async () => {
    const user = userEvent.setup();
    setup({ cycleId: "cycle-1" });
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await user.click(await screen.findByRole("menuitem", { name: "Create work item" }));
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-initial", JSON.stringify({ stateId: TODO, cycleId: "cycle-1" }));
    expect(mocks.attachInput).toHaveBeenCalledWith(
      expect.objectContaining({ cycleId: "cycle-1", moduleId: undefined })
    );
  });

  it("adds an existing work item from the cycle menu and closes on success", async () => {
    const user = userEvent.setup();
    setup({ cycleId: "cycle-1" });
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await user.click(await screen.findByRole("menuitem", { name: "Add an existing work item" }));
    const modal = await screen.findByTestId("add-existing-modal");
    expect(modal.getAttribute("data-exclude")).toContain(ISSUE);
    const props = mocks.addExisting.mock.calls[0]?.[0] as { excludeCycleMembers: boolean };
    expect(props.excludeCycleMembers).toBe(true);
    await user.click(screen.getByRole("button", { name: "Attach picked" }));
    await waitFor(() => expect(mocks.attachExisting).toHaveBeenCalledWith(["picked-1"]));
    await waitFor(() => expect(screen.queryByTestId("add-existing-modal")).not.toBeInTheDocument());
  });

  it("keeps the attach modal open when the attach reports back", async () => {
    const user = userEvent.setup();
    mocks.attachExisting.mockResolvedValueOnce(false);
    setup({ cycleId: "cycle-1" });
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await user.click(await screen.findByRole("menuitem", { name: "Add an existing work item" }));
    await screen.findByTestId("add-existing-modal");
    await user.click(screen.getByRole("button", { name: "Attach picked" }));
    await waitFor(() => expect(mocks.attachExisting).toHaveBeenCalled());
    expect(screen.getByTestId("add-existing-modal")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close attach" }));
    expect(screen.queryByTestId("add-existing-modal")).not.toBeInTheDocument();
  });

  it("offers the menu with the module preset in module context", async () => {
    const user = userEvent.setup();
    setup({ moduleId: "module-1" });
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await user.click(await screen.findByRole("menuitem", { name: "Create work item" }));
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-initial", JSON.stringify({ stateId: TODO, moduleIds: ["module-1"] }));
  });

  it("excludes the route module when adding existing in module context", async () => {
    const user = userEvent.setup();
    setup({ moduleId: "module-1" });
    await user.click(within(column("Todo")).getByTestId("kanban-header-create"));
    await user.click(await screen.findByRole("menuitem", { name: "Add an existing work item" }));
    await screen.findByTestId("add-existing-modal");
    const props = mocks.addExisting.mock.calls[0]?.[0] as {
      excludeModuleId?: string;
      excludeCycleMembers: boolean;
    };
    expect(props.excludeModuleId).toBe("module-1");
    expect(props.excludeCycleMembers).toBe(false);
  });

  it("disables the add control without edit standing or in creator groupings", async () => {
    const user = userEvent.setup();
    setup({ canEdit: false });
    const readOnly = within(column("Todo")).getByTestId("kanban-header-create");
    expect(readOnly).toHaveAttribute("aria-disabled", "true");
    expect(readOnly).toHaveAttribute("title", "New work items are read-only for you");
    await user.click(readOnly);
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
  });

  it("disables the add control for creator groupings", () => {
    setup({ prefs: prefs({ groupBy: "created_by" }) });
    const trigger = within(column("Todo")).getByTestId("kanban-header-create");
    expect(trigger).toHaveAttribute("aria-disabled", "true");
    expect(trigger).toHaveAttribute("title", "Creation is off in this grouping");
  });

  it("creates from the swimlane group headers with the column payload", async () => {
    const user = userEvent.setup();
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "labels" }) });
    const headers = screen.getAllByTestId("kanban-group-header");
    await user.click(within(headers[0] as HTMLElement).getByTestId("kanban-header-create"));
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-initial", JSON.stringify({ stateId: TODO }));
  });
});

describe("KanbanBoard drag and drop", () => {
  it("reorders within the column and highlights the dropped card", () => {
    setup();
    stubFlatGeometry();
    const card = cardByName("Fix login");
    pressCard(card);
    expect(screen.getByTestId("kanban-delete-zone")).toHaveAttribute("data-active", "true");
    expect(card).toHaveAttribute("data-dragging", "true");
    fireEvent(document, pointer("pointermove", 100, 240));
    expect(screen.getByTestId("kanban-drop-indicator")).toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 100, 240));
    expect(mocks.moveCard).toHaveBeenCalledTimes(1);
    expect(mocks.moveCard).toHaveBeenCalledWith({
      issueId: ISSUE,
      plan: { patch: { sort_order: 65536 }, cycle: null, modules: null, reorderOnly: true },
      currentModuleIds: [],
    });
    expect(screen.getByTestId("kanban-delete-zone").getAttribute("data-active")).toBeNull();
    const dropped = screen
      .getAllByTestId("kanban-card")
      .find((entry) => entry.getAttribute("data-issue-id") === ISSUE) as HTMLElement;
    expect(dropped.className).toContain("ring-(--accent)");
  });

  it("clears the dropped highlight after its moment and on the next drag", () => {
    vi.useFakeTimers();
    setup();
    stubFlatGeometry();
    const ringOf = (id: string) =>
      screen.getAllByTestId("kanban-card").find((entry) => entry.getAttribute("data-issue-id") === id)?.className ?? "";
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointerup", 100, 240));
    expect(mocks.moveCard).toHaveBeenCalledTimes(1);
    expect(ringOf(ISSUE)).toContain("ring-(--accent)");
    pressCard(cardByName("Ship it"));
    expect(ringOf(ISSUE)).not.toContain("ring-(--accent)");
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).toHaveBeenCalledTimes(2);
    expect(ringOf("issue-2")).toContain("ring-(--accent)");
    act(() => vi.advanceTimersByTime(DROPPED_HIGHLIGHT_MS));
    expect(ringOf("issue-2")).not.toContain("ring-(--accent)");
  });

  it("moves the card across columns with the destination value", () => {
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 400, 300));
    expect(screen.getByTestId("kanban-drop-indicator")).toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).toHaveBeenCalledWith({
      issueId: ISSUE,
      plan: {
        patch: { state_id: DONE, sort_order: 65535 },
        cycle: null,
        modules: null,
        reorderOnly: false,
      },
      currentModuleIds: [],
    });
  });

  it("seats the card above its target on the top half", () => {
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Ship it"));
    fireEvent(document, pointer("pointermove", 100, 80));
    const indicator = screen.getByTestId("kanban-drop-indicator");
    const todoCards = within(column("Todo")).getAllByTestId("kanban-card");
    expect(indicator.compareDocumentPosition(todoCards[0] as HTMLElement)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    fireEvent(document, pointer("pointerup", 100, 80));
    expect(mocks.moveCard).toHaveBeenCalledTimes(1);
    expect(mocks.moveCard).toHaveBeenCalledWith(
      expect.objectContaining({ issueId: "issue-2", plan: expect.objectContaining({ reorderOnly: true }) })
    );
  });

  it("leaves other columns' cards alone while a card hovers", () => {
    const board = flatBoard();
    board.columns[1] = { key: DONE, title: "Done", total: 1 };
    board.lanes[0]!.cells[DONE] = {
      columnKey: DONE,
      rows: [row({ id: "issue-3", name: "Done deal", sequence_id: 44 })],
      total: 1,
    };
    mocks.board = board;
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 100, 240));
    expect(screen.getByTestId("kanban-drop-indicator")).toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 100, 240));
    expect(mocks.moveCard).toHaveBeenCalledTimes(1);
  });

  it("stays silent on its own seat and over empty chrome", () => {
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointerup", 90, 90));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).not.toHaveBeenCalled();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 900, 900));
    fireEvent(document, pointer("pointerup", 900, 900));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).not.toHaveBeenCalled();
  });

  it("hands delete-zone drops to the delete confirmation", async () => {
    const user = userEvent.setup();
    mocks.projects = [
      {
        id: PROJECT,
        identifier: "WEB",
        name: "Web",
        member_role: 15,
        archived_at: null,
        cycle_view: true,
        module_view: true,
      },
    ];
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 400, 20));
    expect(screen.getByTestId("kanban-delete-zone")).toHaveAttribute("data-hovered", "true");
    fireEvent(document, pointer("pointerup", 400, 20));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    await screen.findByTestId("delete-dialog");
    expect(mocks.deleteDialog).toHaveBeenCalledWith(
      expect.objectContaining({
        scope: { workspaceSlug: "acme", projectId: PROJECT, issueId: ISSUE },
        projectIdentifier: "WEB",
        sequenceId: 42,
        viewerId: "me-1",
        creatorId: null,
        projectRole: 15,
      })
    );
    await user.click(screen.getByRole("button", { name: "Close delete" }));
    expect(screen.queryByTestId("delete-dialog")).not.toBeInTheDocument();
  });

  it("deletes without viewer or project standing when both are missing", async () => {
    mocks.me = null;
    mocks.projects = [];
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointerup", 400, 20));
    await screen.findByTestId("delete-dialog");
    expect(mocks.deleteDialog).toHaveBeenCalledWith(expect.objectContaining({ viewerId: null, projectRole: null }));
  });

  it("refuses drops under a non-manual order with the overlay and a notice", () => {
    setup({ prefs: prefs({ orderBy: "-created_at" }) });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 400, 300));
    expect(screen.getByTestId("kanban-drop-overlay")).toHaveTextContent("Ordered by Last created");
    expect(screen.queryByTestId("kanban-drop-indicator")).not.toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).toHaveBeenCalledWith(
      "Cannot move the work item",
      expect.stringMatching(/ordered by Last created/i)
    );
  });

  it("refuses undraggable groupings without an overlay", () => {
    setup({ prefs: prefs({ groupBy: "created_by" }) });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 400, 300));
    expect(screen.queryByTestId("kanban-drop-overlay")).not.toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).toHaveBeenCalledWith("Cannot move the work item", expect.any(String));
  });

  it("refuses a same-cell reorder under a non-manual order", () => {
    setup({ prefs: prefs({ orderBy: "-created_at" }) });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 100, 240));
    expect(screen.getByTestId("kanban-drop-overlay")).toHaveTextContent("Ordered by Last created");
    fireEvent(document, pointer("pointerup", 100, 240));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).toHaveBeenCalledWith(
      "Cannot move the work item",
      expect.stringMatching(/ordered by Last created/i)
    );
  });

  it("refuses a same-cell reorder in undraggable groupings", () => {
    setup({ prefs: prefs({ groupBy: "created_by" }) });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 100, 240));
    fireEvent(document, pointer("pointerup", 100, 240));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).toHaveBeenCalledWith("Cannot move the work item", expect.any(String));
  });

  it("stays silent dropping back onto its own seat under a refused order", () => {
    setup({ prefs: prefs({ orderBy: "-created_at" }) });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointerup", 90, 90));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).not.toHaveBeenCalled();
  });

  it("refuses completed cycle columns but lets cards leave them", () => {
    mocks.board = {
      columns: [
        { key: "cycle-past", title: "Past", total: 1 },
        { key: "None", title: "No cycle", total: 1 },
      ],
      lanes: [
        {
          key: "",
          title: "",
          total: 2,
          cells: {
            "cycle-past": { columnKey: "cycle-past", rows: [row({ name: "Old" })], total: 1 },
            None: {
              columnKey: "None",
              rows: [row({ id: "issue-9", name: "Fresh", sequence_id: 44 })],
              total: 1,
            },
          },
        },
      ],
      subGrouped: false,
      total: 2,
    } as BoardData;
    setup({ prefs: prefs({ groupBy: "cycle" }) });
    const past = screen
      .getAllByTestId("kanban-column")
      .find((element) => element.getAttribute("data-column-key") === "cycle-past") as HTMLElement;
    const none = screen
      .getAllByTestId("kanban-column")
      .find((element) => element.getAttribute("data-column-key") === "None") as HTMLElement;
    stubRect(past, 0, 0, 350, 600);
    stubRect(none, 350, 0, 700, 600);
    pressCard(cardByName("Fresh"));
    fireEvent(document, pointer("pointermove", 100, 300));
    expect(screen.getByTestId("kanban-drop-overlay")).toHaveTextContent("Cannot move to a completed cycle");
    fireEvent(document, pointer("pointerup", 100, 300));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).toHaveBeenCalledWith("Cannot move the work item", expect.stringMatching(/complet/i));
    pressCard(cardByName("Old"));
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).toHaveBeenCalledTimes(1);
    expect(mocks.moveCard).toHaveBeenCalledWith(
      expect.objectContaining({
        plan: expect.objectContaining({ cycle: { add: null, remove: "cycle-past" } }),
      })
    );
  });

  it("surfaces the edition workflow reason over the blocked column", () => {
    setup({ dropBlockedReason: () => "Needs a review first" });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 400, 300));
    expect(screen.getByTestId("kanban-drop-overlay")).toHaveTextContent("Needs a review first");
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.notifyInfo).toHaveBeenCalledWith("Cannot move the work item", "Needs a review first");
    expect(mocks.moveCard).not.toHaveBeenCalled();
  });

  it("moves on when the edition seam allows the column", () => {
    setup({ dropBlockedReason: () => undefined });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 400, 300));
    expect(screen.queryByTestId("kanban-drop-overlay")).not.toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).toHaveBeenCalledTimes(1);
  });

  it("never starts a drag on touch layouts or read-only boards", () => {
    mocks.touch = true;
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    expect(screen.getByTestId("kanban-delete-zone").getAttribute("data-active")).toBeNull();
  });

  it("never starts a drag without edit standing", () => {
    setup({ canEdit: false });
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    expect(screen.getByTestId("kanban-delete-zone").getAttribute("data-active")).toBeNull();
  });

  it("ignores drops over rows the board no longer shows", () => {
    setup();
    stubFlatGeometry();
    const stray = document.createElement("article");
    stray.setAttribute("data-testid", "kanban-card");
    stray.setAttribute("data-issue-id", "stray");
    stray.setAttribute("data-group-key", "ghost");
    stray.setAttribute("data-lane-key", "");
    stubRect(stray, 750, 100, 1050, 200);
    document.body.appendChild(stray);
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointerup", 800, 150));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).not.toHaveBeenCalled();
    stray.remove();
  });

  it("holds the drop when the plan comes back empty", () => {
    mocks.planBoardMove.mockReturnValueOnce(null);
    setup();
    stubFlatGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointerup", 400, 300));
    expect(mocks.moveCard).not.toHaveBeenCalled();
  });
});

function swimlaneCell(laneKey: string, columnKey: string): HTMLElement {
  const lane = screen.getAllByTestId("kanban-lane").find((entry) => entry.getAttribute("data-lane-key") === laneKey);
  if (lane === undefined) throw new Error(`no lane ${laneKey}`);
  const cell = within(lane)
    .getAllByTestId("kanban-cell")
    .find((entry) => entry.getAttribute("data-column-key") === columnKey);
  if (cell === undefined) throw new Error(`no cell ${laneKey}:${columnKey}`);
  return cell;
}

describe("KanbanBoard swimlane drag and drop", () => {
  function stubSwimlaneGeometry(): void {
    stubRect(swimlaneCell("bug", TODO), 220, 44, 570, 300);
    stubRect(swimlaneCell("bug", DONE), 570, 44, 920, 300);
    stubRect(swimlaneCell("None", TODO), 220, 300, 570, 560);
    stubRect(swimlaneCell("None", DONE), 570, 300, 920, 560);
    const cards = within(swimlaneCell("bug", TODO)).getAllByTestId("kanban-card");
    stubRect(cards[0] as HTMLElement, 228, 60, 562, 150);
    stubRect(screen.getByTestId("kanban-delete-zone"), 300, 0, 500, 40);
  }

  it("moves the card across columns inside its lane", () => {
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "labels" }) });
    stubSwimlaneGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 600, 200));
    expect(screen.getByTestId("kanban-drop-indicator")).toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 600, 200));
    expect(mocks.moveCard).toHaveBeenCalledWith({
      issueId: ISSUE,
      plan: {
        patch: { state_id: DONE, sort_order: 65536 },
        cycle: null,
        modules: null,
        reorderOnly: false,
      },
      currentModuleIds: [],
    });
  });

  it("moves the card across lanes and clears the untouched lane", () => {
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "labels" }) });
    stubSwimlaneGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 300, 400));
    fireEvent(document, pointer("pointerup", 300, 400));
    expect(mocks.moveCard).toHaveBeenCalledWith({
      issueId: ISSUE,
      plan: {
        patch: { label_ids: [], sort_order: 65536 },
        cycle: null,
        modules: null,
        reorderOnly: false,
      },
      currentModuleIds: [],
    });
  });

  it("refuses the lane move when the lane dimension cannot drag", () => {
    mocks.board = swimlaneBoard();
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "created_by" }) });
    stubSwimlaneGeometry();
    pressCard(cardByName("Fix login"));
    fireEvent(document, pointer("pointermove", 300, 400));
    expect(screen.queryByTestId("kanban-drop-overlay")).not.toBeInTheDocument();
    fireEvent(document, pointer("pointerup", 300, 400));
    expect(mocks.moveCard).not.toHaveBeenCalled();
    expect(mocks.notifyInfo).toHaveBeenCalledWith("Cannot move the work item", expect.any(String));
  });
});

describe("KanbanBoard pagination", () => {
  function installObserver(): Array<(entries: Array<{ isIntersecting: boolean }>) => void> {
    const callbacks: Array<(entries: Array<{ isIntersecting: boolean }>) => void> = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: (entries: Array<{ isIntersecting: boolean }>) => void) {
          callbacks.push(callback);
        }
        observe(): void {}
        disconnect(): void {}
      }
    );
    return callbacks;
  }

  it("loads the next column page once its sentinel nears the viewport", () => {
    const callbacks = installObserver();
    mocks.moreColumns = [TODO];
    setup();
    expect(screen.getByTestId("kanban-column-sentinel")).toBeInTheDocument();
    act(() => callbacks.forEach((callback) => callback([{ isIntersecting: true }])));
    expect(mocks.loadMoreColumn).toHaveBeenCalledWith(TODO);
  });

  it("shows skeleton loaders while a column page flies and hides the sentinel", () => {
    mocks.moreColumns = [TODO];
    mocks.fetchingColumns = [TODO];
    setup();
    expect(within(column("Todo")).getByTestId("kanban-column-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("kanban-column-sentinel")).not.toBeInTheDocument();
  });

  it("loads the next cell page behind its explicit entry", async () => {
    const user = userEvent.setup();
    mocks.board = swimlaneBoard();
    mocks.moreCells = [`bug:${TODO}`];
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "labels" }) });
    const cell = swimlaneCell("bug", TODO);
    await user.click(within(cell).getByTestId("kanban-load-more"));
    expect(mocks.loadMoreCell).toHaveBeenCalledWith(TODO, "bug");
    expect(within(swimlaneCell("None", TODO)).queryByTestId("kanban-load-more")).not.toBeInTheDocument();
  });

  it("shows skeleton loaders while a cell page flies", () => {
    mocks.board = swimlaneBoard();
    mocks.moreCells = [`bug:${TODO}`];
    mocks.fetchingCells = [`bug:${TODO}`];
    setup({ prefs: prefs({ groupBy: "state", subGroupBy: "labels" }) });
    expect(within(swimlaneCell("bug", TODO)).getByTestId("kanban-column-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("kanban-load-more")).not.toBeInTheDocument();
  });
});

describe("KanbanBoard virtualization", () => {
  it("holds far cards as marked placeholders until they near the viewport", () => {
    const callbacks: Array<(entries: Array<{ isIntersecting: boolean }>) => void> = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: (entries: Array<{ isIntersecting: boolean }>) => void) {
          callbacks.push(callback);
        }
        observe(): void {}
        disconnect(): void {}
      }
    );
    setup();
    const shells = screen.getAllByTestId("kanban-card");
    expect(shells).toHaveLength(2);
    expect(shells[0]).toHaveAttribute("data-virtualized", "true");
    expect(shells[0]).toHaveAttribute("data-issue-id", ISSUE);
    expect(screen.queryByTestId("kanban-card-name")).not.toBeInTheDocument();
    act(() => callbacks.forEach((callback) => callback([{ isIntersecting: true }])));
    expect(screen.getAllByTestId("kanban-card-name")).toHaveLength(2);
  });
});
