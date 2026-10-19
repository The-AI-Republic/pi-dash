// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  pages: vi.fn(),
  refs: vi.fn(),
  toggle: vi.fn(),
  create: vi.fn(),
  addExisting: vi.fn(),
  addToCycle: vi.fn(),
  addToModule: vi.fn(),
  createMutation: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
  permissions: { canEdit: true },
}));

vi.mock("./pages.js", () => ({ useListPages: mocks.pages }));
vi.mock("./reference.js", () => ({ useListReferences: mocks.refs }));
vi.mock("./collapse.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./collapse.js")>()),
  useToggleCollapsedGroup: () => ({ toggleCollapsedGroup: mocks.toggle }),
}));
vi.mock("../api/create.js", () => ({ useCreateWorkItem: () => ({ mutateAsync: mocks.createMutation }) }));
vi.mock("./QuickAdd.js", () => ({
  QuickAdd: (props: {
    disabled: boolean;
    disabledReason?: string;
    prefill: Record<string, unknown>;
    groupKey?: string;
  }) => (
    <div
      data-testid="quick-add-probe"
      data-disabled={String(props.disabled)}
      data-reason={props.disabledReason ?? ""}
      data-prefill={JSON.stringify(props.prefill)}
      data-group={props.groupKey ?? ""}
    />
  ),
}));
vi.mock("./AddExistingModal.js", () => ({
  AddExistingModal: (props: {
    onAttach: (ids: string[]) => Promise<void>;
    excludeCycleMembers?: boolean;
    excludeModuleId?: string;
  }) => (
    <div
      data-testid="add-existing-modal"
      data-cycle={String(props.excludeCycleMembers ?? false)}
      data-module={props.excludeModuleId ?? ""}
    >
      <button type="button" onClick={() => void props.onAttach(["one"]).catch(() => undefined)}>
        Attach one
      </button>
      <button type="button" onClick={() => void props.onAttach(["one", "two"]).catch(() => undefined)}>
        Attach two
      </button>
    </div>
  ),
}));
vi.mock("./ListRow.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./ListRow.js")>()),
  ListRow: (props: {
    issue: IssueListItem;
    compact?: boolean;
    onOpenPeek: (key: string, title: string) => void;
    hrefFor: (row: IssueListItem) => string;
  }) => (
    <div data-testid="row-probe" data-compact={String(props.compact ?? false)} data-href={props.hrefFor(props.issue)}>
      <button type="button" onClick={() => props.onOpenPeek(`WEB-${props.issue.sequence_id}`, props.issue.name)}>
        {props.issue.name}
      </button>
      <button type="button" onClick={() => props.onOpenPeek("WEB-99", "Nested child")}>
        Nested child
      </button>
    </div>
  ),
}));
vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));
vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  usePermissions: () => mocks.permissions,
}));
vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  addIssueToCycle: mocks.addToCycle,
  addIssuesToModule: mocks.addToModule,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/index.js";
import { issueKeys } from "../api/keys.js";
import { entityPrefsKeyPart } from "../prefs/prefsQuery.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity, PrefsRecord } from "../prefs/entities.js";
import { groupedSections } from "./grouping.js";
import { ListLayout, type ListLayoutProps } from "./ListLayout.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
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

function prefs(overrides: Partial<DisplayPrefs> = {}): DisplayPrefs {
  return {
    layout: "list",
    groupBy: null,
    subGroupBy: null,
    orderBy: "sort_order",
    showEmptyGroups: false,
    showSubIssues: false,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
    ...overrides,
  };
}

function pageState(overrides: Record<string, unknown> = {}) {
  return {
    sections: [{ key: "all", title: "All work items", rows: [row()], total: 1, value: null }],
    total: 1,
    grouped: false,
    isPending: false,
    isFetching: false,
    hasMore: false,
    loadMore: vi.fn(),
    loadMoreSection: vi.fn(),
    sectionMore: () => false,
    ...overrides,
  };
}

function refs() {
  return {
    labels: {},
    stateRows: [
      {
        id: STATE,
        project_id: PROJECT,
        workspace_id: "",
        name: "Todo",
        color: "#fff",
        group: "unstarted",
        default: false,
        description: "",
        sequence: 1,
      },
    ],
    stateNames: new Map([[STATE, "Todo"]]),
    assigneeNames: new Map(),
    labelNames: new Map(),
    cycleNames: new Map(),
    moduleNames: new Map(),
    projectNames: new Map(),
    openCycleIds: new Set<string>(),
    projects: [],
    isLoading: false,
  };
}

function seed(queryClient: QueryClient, entity: IssuesEntity, filters: Record<string, unknown> = {}) {
  const record: PrefsRecord = {
    filters,
    display_filters: {},
    display_properties: {},
    rich_filters: {},
  };
  queryClient.setQueryData(issueKeys.prefs("acme", ...entityPrefsKeyPart(entity)), record);
  queryClient.setQueryData(projectKeys.list("acme"), [
    {
      id: PROJECT,
      name: "Website",
      identifier: "WEB",
      sort_order: 1,
      logo_props: {},
      member_role: 15,
      intake_count: 0,
      archived_at: null,
      workspace: "623e4567-e89b-12d3-a456-426614174005",
      cycle_view: true,
      issue_views_view: true,
      module_view: true,
      page_view: true,
      inbox_view: true,
      is_default: false,
      guest_view_all_features: false,
      project_lead: null,
      network: 2,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-02-03T04:05:06Z",
      created_by: null,
      updated_by: null,
    },
  ]);
  queryClient.setQueryData(projectKeys.details("acme"), [
    {
      id: PROJECT,
      created_at: "2024-01-02T03:04:05Z",
      updated_at: "2024-02-03T04:05:06Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      workspace: "623e4567-e89b-12d3-a456-426614174005",
      name: "Website",
      description: "",
      description_text: null,
      description_html: null,
      network: 2,
      identifier: "WEB",
      default_assignee: null,
      project_lead: null,
      emoji: null,
      icon_prop: null,
      module_view: true,
      cycle_view: true,
      issue_views_view: true,
      page_view: true,
      intake_view: true,
      inbox_view: true,
      is_time_tracking_enabled: false,
      is_issue_type_enabled: false,
      is_default: false,
      guest_view_all_features: false,
      members_can_edit_states: false,
      cover_image: null,
      cover_image_asset: null,
      cover_image_url: null,
      estimate: null,
      archive_in: 0,
      close_in: 0,
      logo_props: {},
      default_state: STATE,
      archived_at: null,
      timezone: "UTC",
      external_source: null,
      external_id: null,
      repo_url: "",
      base_branch: "main",
      is_favorite: false,
      sort_order: 1,
      member_role: 15,
      anchor: null,
      members: [],
      next_work_item_sequence: 43,
      agent_executor_options: [],
      default_agent_executor: "",
    },
  ]);
}

function renderLayout(overrides: Partial<ListLayoutProps> = {}, filters: Record<string, unknown> = {}) {
  const entity = overrides.entity ?? ({ scope: "project", projectId: PROJECT } as const);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  seed(queryClient, entity, filters);
  const props: ListLayoutProps = {
    workspaceSlug: "acme",
    entity,
    projectId: PROJECT,
    projectIdentifier: "WEB",
    prefs: prefs(),
    ...overrides,
  };
  const view = render(
    <QueryClientProvider client={queryClient}>
      <ListLayout {...props} />
    </QueryClientProvider>
  );
  return { props, view, queryClient };
}

// jsdom reports zero element sizes, so the flat VirtualList would compute
// an empty window. Give the scroll region and rows a height so rows mount.
const realHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region" || this.getAttribute("data-testid") === "list-scroll") return 400;
      if (this.hasAttribute("data-index")) return 44;
      return realHeight?.get?.call(this) ?? 0;
    },
  });
  createAppClient("https://api.test", (() => Promise.reject(new Error("unexpected network"))) as never);
  mocks.permissions = { canEdit: true };
  mocks.pages.mockReturnValue(pageState());
  mocks.refs.mockReturnValue(refs());
  mocks.createMutation.mockResolvedValue({ id: ISSUE });
  mocks.addToCycle.mockResolvedValue(undefined);
  mocks.addToModule.mockResolvedValue(undefined);
  class TestIntersectionObserver {
    observe(): void {}
    disconnect(): void {}
  }
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
});

afterEach(() => {
  if (realHeight) Object.defineProperty(HTMLElement.prototype, "offsetHeight", realHeight);
  resetAppClient();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("ListLayout", () => {
  it("renders the flat section, row, quick-add and link target", async () => {
    renderLayout();
    expect(screen.getByText("All work items")).toBeInTheDocument();
    expect(screen.getByTestId("row-probe")).toHaveAttribute("data-href", "/acme/browse/WEB-42");
    const quickAdd = await screen.findByTestId("quick-add-probe");
    expect(quickAdd).toHaveAttribute("data-disabled", "false");
    expect(quickAdd).toHaveAttribute("data-prefill", "{}");
  });

  it("passes grouping, counts and prefill to headers and quick-add", () => {
    const sections = groupedSections(
      { results: { [STATE]: { results: [row()], total_results: 1 }, empty: { results: [], total_results: 0 } } },
      new Map([[STATE, "Todo"]]),
      { showEmptyGroups: true, dimension: "state" }
    );
    mocks.pages.mockReturnValue(
      pageState({ sections, grouped: true, sectionMore: (section: { rows: unknown[] }) => section.rows.length === 1 })
    );
    renderLayout({ prefs: prefs({ groupBy: "state", showEmptyGroups: true }) });
    expect(screen.getByText("Todo")).toBeInTheDocument();
    expect(screen.getByText("empty")).toBeInTheDocument();
    expect(screen.getAllByTestId("quick-add-probe")[0]).toHaveAttribute(
      "data-prefill",
      JSON.stringify({ state_id: STATE })
    );
    expect(screen.getAllByTestId("quick-add-probe")[0]).toHaveAttribute("data-group", STATE);
    expect(screen.getByRole("button", { name: "Load more" })).toBeInTheDocument();
  });

  it("hides rows and quick-add for a collapsed section", () => {
    renderLayout({}, { kanban_filters: { group_by: ["all"], sub_group_by: [] } });
    expect(screen.queryByTestId("row-probe")).not.toBeInTheDocument();
    expect(screen.queryByTestId("quick-add-probe")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("list-group-header"));
    expect(mocks.toggle).toHaveBeenCalledWith("group_by", "all");
  });

  it("renders the loading skeleton while pages resolve", () => {
    mocks.pages.mockReturnValue(pageState({ isPending: true, sections: [] }));
    renderLayout();
    expect(screen.getByTestId("list-loading")).toBeInTheDocument();
  });

  it("disables creation for guests, completed cycles and creator grouping", () => {
    mocks.permissions = { canEdit: false };
    const guest = renderLayout();
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute(
      "data-reason",
      "New work items are read-only for you"
    );
    expect(screen.queryByRole("button", { name: "Add to All work items" })).not.toBeInTheDocument();
    guest.view.unmount();
    mocks.permissions = { canEdit: true };
    const completed = renderLayout({ completedCycle: true });
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute("data-reason", "This cycle is complete");
    completed.view.unmount();
    renderLayout({ prefs: prefs({ groupBy: "created_by" }) });
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute("data-reason", "Creation is off in this grouping");
  });

  it("opens the create dialog and submits prefilled fields", async () => {
    const user = userEvent.setup();
    renderLayout({ cycleId: CYCLE, moduleId: MODULE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Create work item" }));
    await user.type(await screen.findByRole("textbox", { name: "Title" }), "New item");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(mocks.createMutation).toHaveBeenCalled());
    expect(mocks.createMutation).toHaveBeenCalledWith({
      projectId: PROJECT,
      payload: { name: "New item", state_id: STATE, cycle_id: CYCLE, module_ids: [MODULE] },
    });
    expect(mocks.addToCycle).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, CYCLE, ISSUE);
    expect(mocks.addToModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, MODULE, [ISSUE]);
  });

  it("offers add-existing in cycle/module context and attaches to the entity", async () => {
    const user = userEvent.setup();
    renderLayout({ moduleId: MODULE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    await user.click(screen.getByRole("menuitem", { name: "Add an existing work item" }));
    expect(await screen.findByTestId("add-existing-modal")).toHaveAttribute("data-module", MODULE);
    await user.click(screen.getByRole("button", { name: "Attach one" }));
    await waitFor(() =>
      expect(mocks.addToModule).toHaveBeenCalledWith(expect.anything(), "acme", PROJECT, MODULE, ["one"])
    );
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Work item added");
  });

  it("opens create directly for a plain-project cycle or module bucket", async () => {
    const user = userEvent.setup();
    const cycleSections = groupedSections(
      { results: { [CYCLE]: { results: [], total_results: 0 } } },
      new Map([[CYCLE, "Sprint"]]),
      { showEmptyGroups: true, dimension: "cycle" }
    );
    mocks.pages.mockReturnValue(pageState({ sections: cycleSections, grouped: true }));
    renderLayout({ prefs: prefs({ groupBy: "cycle", showEmptyGroups: true }) });
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add to Sprint" }));
    expect(screen.getByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    await user.type(await screen.findByRole("textbox", { name: "Title" }), "Bucket item");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(mocks.createMutation).toHaveBeenCalledWith({
        projectId: PROJECT,
        payload: { name: "Bucket item", state_id: STATE, cycle_id: CYCLE },
      })
    );
    expect(mocks.addToCycle).not.toHaveBeenCalled();
  });

  it("excludes current cycle members from the add-existing search", async () => {
    const user = userEvent.setup();
    renderLayout({ cycleId: CYCLE, prefs: prefs({ groupBy: "cycle" }) });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    await user.click(await screen.findByRole("menuitem", { name: "Add an existing work item" }));
    expect(await screen.findByTestId("add-existing-modal")).toHaveAttribute("data-cycle", "true");
    expect(screen.getByTestId("add-existing-modal")).toHaveAttribute("data-module", "");
  });

  it("excludes rows already in the module from the add-existing search", async () => {
    const user = userEvent.setup();
    renderLayout({ moduleId: MODULE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    await user.click(await screen.findByRole("menuitem", { name: "Add an existing work item" }));
    expect(await screen.findByTestId("add-existing-modal")).toHaveAttribute("data-module", MODULE);
  });

  it("reports attach failures without closing the modal", async () => {
    const user = userEvent.setup();
    mocks.addToModule.mockRejectedValue(new Error("bridge down"));
    renderLayout({ moduleId: MODULE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    await user.click(screen.getByRole("menuitem", { name: "Add an existing work item" }));
    await user.click(screen.getByRole("button", { name: "Attach one" }));
    await waitFor(() =>
      expect(mocks.notifyError).toHaveBeenCalledWith(
        "Couldn't add the work items",
        "Check your connection and try again."
      )
    );
    expect(screen.getByTestId("add-existing-modal")).toBeInTheDocument();
  });

  it("opens peek and clears it via Escape or the close control", async () => {
    const onPeekChange = vi.fn();
    const user = userEvent.setup();
    const { view } = renderLayout({ peekKey: null, onPeekChange });
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    expect(onPeekChange).toHaveBeenCalledWith("WEB-42");
    expect(screen.queryByLabelText("Work item preview")).not.toBeInTheDocument();
    view.unmount();
    renderLayout({ peekKey: "WEB-42", onPeekChange });
    expect(await screen.findByLabelText("Work item preview")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("supports uncontrolled peek state", async () => {
    const user = userEvent.setup();
    renderLayout();
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    expect(await screen.findByLabelText("Work item preview")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.queryByLabelText("Work item preview")).not.toBeInTheDocument();
  });

  it("opens peek for a nested row key that is not in the top-level pages", async () => {
    renderLayout();
    fireEvent.click(screen.getByRole("button", { name: "Nested child" }));
    expect(await screen.findByLabelText("Work item preview")).toBeInTheDocument();
    expect(screen.getByLabelText("Work item title")).toHaveValue("Nested child");
  });

  it("passes compact density through and keeps the flat sentinel inside the scroll region", () => {
    mocks.pages.mockReturnValue(pageState({ hasMore: true }));
    renderLayout({ compact: true });
    expect(screen.getByTestId("row-probe")).toHaveAttribute("data-compact", "true");
    const scroll = screen.getByTestId("list-scroll");
    const sentinel = screen.getByTestId("list-sentinel");
    expect(scroll).toContainElement(sentinel);
    expect(screen.getByRole("region", { name: "Issues" })).toContainElement(sentinel);
  });

  it("disables quick-add when the view flag is off or workflow blocks creation", async () => {
    const first = renderLayout({ enableQuickAdd: false });
    expect(await screen.findByTestId("quick-add-probe")).toHaveAttribute("data-disabled", "true");
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute("data-reason", "Quick add is off in this view");
    first.view.unmount();
    renderLayout({ creationBlockedReason: "Workflow blocks this state" });
    expect(await screen.findByTestId("quick-add-probe")).toHaveAttribute("data-disabled", "true");
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute("data-reason", "Workflow blocks this state");
  });

  it("keeps quick-add enabled for an ordinary project view", async () => {
    renderLayout();
    expect(await screen.findByTestId("quick-add-probe")).toHaveAttribute("data-disabled", "false");
  });
});
