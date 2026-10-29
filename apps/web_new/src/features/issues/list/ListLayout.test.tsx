// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const bulkMocks = vi.hoisted(() => ({
  update: vi.fn(),
  remove: vi.fn(),
  archive: vi.fn(),
  subscribe: vi.fn(),
}));

const mocks = vi.hoisted(() => ({
  pages: vi.fn(),
  refs: vi.fn(),
  toggle: vi.fn(),
  create: vi.fn(),
  addExisting: vi.fn(),
  addToCycle: vi.fn(),
  addToModule: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
  permissions: { canEdit: true },
}));

vi.mock("../bulk/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bulk/index.js")>()),
  useBulkMutations: () => ({
    update: { mutateAsync: bulkMocks.update, isPending: false },
    remove: { mutateAsync: bulkMocks.remove, isPending: false },
    archive: { mutateAsync: bulkMocks.archive, isPending: false },
    subscribe: { mutateAsync: bulkMocks.subscribe, isPending: false },
  }),
  BulkToolbar: (props: {
    count: number;
    fromStateIds?: readonly (string | null)[];
    onUpdate: (properties: unknown) => void;
    onDelete: () => void;
    onArchive: () => void;
    onSubscribe: () => void;
    onClear: () => void;
  }) => (
    <div data-testid="bulk-toolbar" data-from-states={JSON.stringify(props.fromStateIds ?? null)}>
      <span>{props.count} selected</span>
      <button type="button" onClick={() => props.onUpdate({ priority: "high" })}>
        Update
      </button>
      <button type="button" onClick={props.onDelete}>
        Delete
      </button>
      <button type="button" onClick={props.onArchive}>
        Archive
      </button>
      <button type="button" onClick={props.onSubscribe}>
        Subscribe
      </button>
      <button type="button" onClick={props.onClear}>
        Clear
      </button>
    </div>
  ),
}));
vi.mock("./pages.js", () => ({ useListPages: mocks.pages }));
vi.mock("./reference.js", () => ({ useListReferences: mocks.refs }));
vi.mock("./collapse.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./collapse.js")>()),
  useToggleCollapsedGroup: () => ({ toggleCollapsedGroup: mocks.toggle }),
}));
vi.mock("../components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: {
    open: boolean;
    workspaceSlug: string;
    projects: Array<{ id: string; name: string; memberRole?: number | null; archived?: boolean }>;
    initialProjectId?: string;
    initialValues?: Record<string, unknown>;
    onCreated?: (id: string) => void;
    onOpenChange?: (open: boolean) => void;
  }) =>
    props.open ? (
      <div
        data-testid="create-dialog"
        role="dialog"
        aria-label="Create new work item"
        data-project={props.initialProjectId ?? ""}
        data-values={JSON.stringify(props.initialValues ?? {})}
        data-projects={JSON.stringify(props.projects)}
      >
        <button type="button" onClick={() => props.onCreated?.("created-1")}>
          create
        </button>
        <button type="button" data-close="" onClick={() => props.onOpenChange?.(false)}>
          close
        </button>
      </div>
    ) : null,
}));
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
    selected?: boolean;
    selectionAvailable?: boolean;
    onSelectionToggle?: (id: string, shiftKey: boolean) => void;
    onExpandedChange?: (id: string, expanded: boolean) => void;
    onChildrenChange?: (id: string, children: readonly IssueListItem[]) => void;
    projectIdentifierFor?: (row: IssueListItem) => string;
    selectionTree?: { expanded: ReadonlySet<string>; children: ReadonlyMap<string, readonly IssueListItem[]> };
    onOpenPeek: (key: string, title: string) => void;
    hrefFor: (row: IssueListItem) => string;
    menuContext?: string;
    menuCycleId?: string | null;
    menuModuleId?: string | null;
    menuCanWrite?: boolean;
  }) => (
    <div
      data-testid="row-probe"
      data-compact={String(props.compact ?? false)}
      data-href={props.hrefFor(props.issue)}
      data-own-key={props.projectIdentifierFor?.(props.issue)}
      data-foreign-key={props.projectIdentifierFor?.(
        row({ id: "foreign-row", project_id: "823e4567-e89b-12d3-a456-426614174008" })
      )}
      data-menu-context={props.menuContext ?? ""}
      data-menu-cycle={props.menuCycleId ?? ""}
      data-menu-module={props.menuModuleId ?? ""}
      data-menu-write={String(props.menuCanWrite ?? "unset")}
    >
      {props.onSelectionToggle === undefined ? null : props.selectionAvailable === false ? (
        <span role="img" aria-label={`Cannot select ${props.issue.sequence_id}`} />
      ) : (
        <input
          type="checkbox"
          aria-label={`Select ${props.issue.sequence_id}`}
          checked={props.selected ?? false}
          onChange={() => props.onSelectionToggle?.(props.issue.id, false)}
        />
      )}
      <button type="button" onClick={() => props.onOpenPeek(props.issue.id, props.issue.name)}>
        {props.issue.name}
      </button>
      <button type="button" onClick={() => props.onOpenPeek("WEB-99", "Nested child")}>
        Nested child
      </button>
      <button
        type="button"
        onClick={() => {
          props.onExpandedChange?.(props.issue.id, true);
          props.onChildrenChange?.(props.issue.id, [
            row({ id: "child-row", sequence_id: 43, parent_id: props.issue.id }),
          ]);
        }}
      >
        Expand selection child
      </button>
      <button
        type="button"
        onClick={() => {
          props.onExpandedChange?.(props.issue.id, false);
          props.onChildrenChange?.(props.issue.id, []);
        }}
      >
        Collapse selection child
      </button>
      <button type="button" onClick={() => props.onExpandedChange?.(props.issue.id, true)}>
        Expand without children
      </button>
      <button type="button" onClick={() => props.onExpandedChange?.(props.issue.id, false)}>
        Collapse without children
      </button>
      <button
        type="button"
        onClick={() => {
          const stable = [row({ id: "stable-child", sequence_id: 44, parent_id: props.issue.id })];
          props.onChildrenChange?.(props.issue.id, stable);
          props.onChildrenChange?.(props.issue.id, stable);
        }}
      >
        Report stable children
      </button>
      {props.selectionTree?.children.get(props.issue.id)?.map((child) => (
        <input
          key={child.id}
          type="checkbox"
          aria-label={`Select ${child.sequence_id}`}
          checked={false}
          onChange={() => props.onSelectionToggle?.(child.id, false)}
        />
      ))}
    </div>
  ),
}));
vi.mock("../peek/ResolvingIssuePeek.js", () => ({
  ResolvingIssuePeek: (props: {
    peeked: string;
    peekProjectId?: string | null;
    onPeekChange?: (selection: { issueId: string; projectId: string; nestingLevel: number } | null) => void;
    onClose: () => void;
  }) => (
    <div aria-label="Work item preview" data-peeked={props.peeked} data-peek-project={props.peekProjectId ?? ""}>
      <button
        type="button"
        onClick={() =>
          props.onPeekChange?.({
            issueId: "123e4567-e89b-12d3-a456-426614174099",
            projectId: "223e4567-e89b-12d3-a456-426614174001",
            nestingLevel: 1,
          })
        }
      >
        Open child
      </button>
      <button
        type="button"
        onClick={() =>
          props.onPeekChange?.({
            issueId: "123e4567-e89b-12d3-a456-426614174000",
            projectId: "223e4567-e89b-12d3-a456-426614174001",
            nestingLevel: 0,
          })
        }
      >
        Back to parent
      </button>
      <button type="button" onClick={props.onClose}>
        Close preview
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
import { entityListsKeyPart } from "../prefs/entities.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity, PrefsRecord } from "../prefs/entities.js";
import { WORKFLOW_FLAG } from "../edition/flags.js";
import { registerEditionIssuesProviders, resetEditionIssuesProviders } from "../edition/providers.js";
import { groupedSections } from "./grouping.js";
import { FRESH_BYPASS_LIMIT } from "../freshness/fresh.js";
import { edition } from "../../../core/edition/index.js";
import { BULK_OPERATIONS_FLAG } from "../bulk/edition.js";
import { ListLayout, type ListLayoutProps } from "./ListLayout.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const STATE = "323e4567-e89b-12d3-a456-426614174002";
const DONE_STATE = "423e4567-e89b-12d3-a456-426614174003";
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
    sectionFetching: () => false,
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
  for (const action of Object.values(bulkMocks)) action.mockResolvedValue(undefined);
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
  mocks.addToCycle.mockResolvedValue(undefined);
  mocks.addToModule.mockResolvedValue(undefined);
  class TestIntersectionObserver {
    observe(): void {}
    disconnect(): void {}
  }
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
});

afterEach(() => {
  delete edition.flags[BULK_OPERATIONS_FLAG];
  delete edition.flags[WORKFLOW_FLAG];
  resetEditionIssuesProviders();
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
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    expect(mocks.pages.mock.results.at(-1)?.value.loadMoreSection).toHaveBeenCalledWith(sections[0]);
  });

  it("hides rows and quick-add for a collapsed section", () => {
    renderLayout({}, { kanban_filters: { group_by: ["all"], sub_group_by: [] } });
    expect(screen.queryByTestId("row-probe")).not.toBeInTheDocument();
    expect(screen.queryByTestId("quick-add-probe")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("list-group-header"));
    expect(mocks.toggle).toHaveBeenCalledWith("group_by", "all");
  });

  it("shows skeleton rows while a grouped section pages and the button again when idle", () => {
    mocks.pages.mockReturnValue(
      pageState({
        grouped: true,
        sections: [{ key: "all", title: "All work items", rows: [row()], total: 51, value: null }],
        sectionMore: () => true,
        sectionFetching: () => true,
      })
    );
    const loading = renderLayout();
    expect(screen.getByTestId("section-loading")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
    loading.view.unmount();
    mocks.pages.mockReturnValue(
      pageState({
        grouped: true,
        sections: [{ key: "all", title: "All work items", rows: [row()], total: 51, value: null }],
        sectionMore: () => true,
      })
    );
    renderLayout();
    expect(screen.queryByTestId("section-loading")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Load more" })).toBeInTheDocument();
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
    const completedModule = renderLayout({ completedModule: true });
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute("data-reason", "This module is complete");
    completedModule.view.unmount();
    renderLayout({ prefs: prefs({ groupBy: "created_by" }) });
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute("data-reason", "Creation is off in this grouping");
  });

  it("honors the host route's project-aware edit gate", () => {
    renderLayout({ canEdit: false });
    expect(screen.getByTestId("quick-add-probe")).toHaveAttribute(
      "data-reason",
      "New work items are read-only for you"
    );
    expect(screen.queryByRole("button", { name: "Add to All work items" })).not.toBeInTheDocument();
  });

  it("opens the shared create dialog with the section's prefill", async () => {
    const user = userEvent.setup();
    renderLayout({ cycleId: CYCLE, moduleId: MODULE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Create work item" }));
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-project", PROJECT);
    expect(JSON.parse(dialog.getAttribute("data-projects") ?? "[]")).toEqual([
      { id: PROJECT, name: "Website", identifier: "WEB", memberRole: 15, archived: false },
    ]);
    expect(JSON.parse(dialog.getAttribute("data-values") ?? "{}")).toEqual({
      cycleId: CYCLE,
      moduleIds: [MODULE],
    });
    await user.click(screen.getByRole("button", { name: "create" }));
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument());
  });

  it("closes the create dialog through its own open-change handler", async () => {
    const user = userEvent.setup();
    renderLayout({ cycleId: CYCLE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    await user.click(await screen.findByRole("menuitem", { name: "Create work item" }));
    const dialog = await screen.findByTestId("create-dialog");
    const close = dialog.querySelector("[data-close]");
    expect(close).not.toBeNull();
    fireEvent.click(close!);
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument());
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
    await waitFor(() => expect(screen.queryByTestId("add-existing-modal")).not.toBeInTheDocument());
  });

  it("carries a label section's membership and assignee value into the create form", async () => {
    const user = userEvent.setup();
    const sections = groupedSections(
      { results: { "label-1": { results: [row()], total_results: 1 } } },
      new Map([["label-1", "Bug"]]),
      { showEmptyGroups: true, dimension: "labels" }
    );
    const assignment = groupedSections(
      { results: { "member-1": { results: [row()], total_results: 1 } } },
      new Map([["member-1", "Ada"]]),
      { showEmptyGroups: true, dimension: "assignees" }
    );
    mocks.pages.mockReturnValue(pageState({ sections, grouped: true }));
    const labelRun = renderLayout({ prefs: prefs({ groupBy: "labels", showEmptyGroups: true }) });
    await user.click(screen.getByRole("button", { name: "Add to Bug" }));
    expect(JSON.parse((await screen.findByTestId("create-dialog")).getAttribute("data-values") ?? "{}")).toEqual({
      labelIds: ["label-1"],
    });
    labelRun.view.unmount();
    mocks.pages.mockReturnValue(pageState({ sections: assignment, grouped: true }));
    const assigneeRun = renderLayout({ prefs: prefs({ groupBy: "assignees", showEmptyGroups: true }) });
    await user.click(screen.getByRole("button", { name: "Add to Ada" }));
    expect(JSON.parse((await screen.findByTestId("create-dialog")).getAttribute("data-values") ?? "{}")).toEqual({
      assigneeIds: ["member-1"],
    });
    assigneeRun.view.unmount();
  });

  it("carries a state or priority section's value into the shared create form", async () => {
    const user = userEvent.setup();
    const stateSections = groupedSections(
      { results: { [STATE]: { results: [row()], total_results: 1 } } },
      new Map([[STATE, "In Progress"]]),
      { showEmptyGroups: true, dimension: "state" }
    );
    mocks.pages.mockReturnValue(pageState({ sections: stateSections, grouped: true }));
    renderLayout({ prefs: prefs({ groupBy: "state", showEmptyGroups: true }) });
    await user.click(screen.getByRole("button", { name: "Add to In Progress" }));
    const dialog = await screen.findByTestId("create-dialog");
    expect(JSON.parse(dialog.getAttribute("data-values") ?? "{}")).toEqual({ stateId: STATE });
    await user.click(screen.getByRole("button", { name: "create" }));
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument());
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
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-project", PROJECT);
    expect(JSON.parse(dialog.getAttribute("data-values") ?? "{}")).toEqual({ cycleId: CYCLE });
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
    renderLayout({ moduleId: MODULE });
    fireEvent.click(screen.getByRole("button", { name: "Add to All work items" }));
    await user.click(screen.getByRole("menuitem", { name: "Add an existing work item" }));
    await screen.findByTestId("add-existing-modal");
    mocks.addToModule.mockRejectedValue(new Error("bridge down"));
    await user.click(screen.getByRole("button", { name: "Attach one" }));
    await waitFor(() =>
      expect(mocks.notifyError).toHaveBeenCalledWith(
        "Couldn't add the work items",
        "Check your connection and try again."
      )
    );
    expect(screen.getByTestId("add-existing-modal")).toBeInTheDocument();
  });

  it("opens the full peek and clears it via the close control", async () => {
    const onPeekChange = vi.fn();
    const user = userEvent.setup();
    const { view } = renderLayout({ peekKey: null, onPeekChange });
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    expect(onPeekChange).toHaveBeenCalledWith({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 });
    expect(screen.queryByLabelText("Work item preview")).not.toBeInTheDocument();
    view.unmount();
    renderLayout({ peekKey: ISSUE, onPeekChange, peekProjectId: null });
    const panel = await screen.findByLabelText("Work item preview");
    expect(panel).toHaveAttribute("data-peeked", ISSUE);
    // Escape belongs to the peek itself (tested there); the layout only
    // forwards the panel's close callback to the controlled key.
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("reports a cross-project row under its own project (ISS-167)", async () => {
    const foreign = "823e4567-e89b-12d3-a456-426614174099";
    mocks.pages.mockReturnValue(
      pageState({
        sections: [{ key: "all", title: "All", rows: [row({ project_id: foreign })], total: 1, value: null }],
      })
    );
    const onPeekChange = vi.fn();
    const user = userEvent.setup();
    renderLayout({ peekKey: null, onPeekChange });
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    expect(onPeekChange).toHaveBeenCalledWith({ issueId: ISSUE, projectId: foreign, nestingLevel: 0 });
  });

  it("supports uncontrolled peek state", async () => {
    const user = userEvent.setup();
    renderLayout();
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    expect(await screen.findByLabelText("Work item preview")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Close preview" }));
    expect(screen.queryByLabelText("Work item preview")).not.toBeInTheDocument();
  });

  it("opens the peek from a shared link on its own (ISS-167)", async () => {
    window.history.replaceState({}, "", `/?peekIssueId=${ISSUE}`);
    renderLayout();
    expect(await screen.findByLabelText("Work item preview")).toHaveAttribute("data-peeked", ISSUE);
    expect(new URLSearchParams(window.location.search).get("peekIssueId")).toBe(ISSUE);
  });

  it("writes a row's raw id, then strips it on close, without a host (ISS-167)", async () => {
    const user = userEvent.setup();
    renderLayout();
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    const opened = new URLSearchParams(window.location.search);
    expect(opened.get("peekIssueId")).toBe(ISSUE);
    expect(opened.get("peekProjectId")).toBeNull();
    expect(opened.get("peekNestingLevel")).toBeNull();
    window.history.replaceState({}, "", `/?peekIssueId=${ISSUE}&layout=compact`);
    await user.click(await screen.findByRole("button", { name: "Close preview" }));
    const closed = new URLSearchParams(window.location.search);
    expect(closed.get("peekIssueId")).toBeNull();
    expect(closed.get("layout")).toBe("compact");
  });

  it("writes the nested level from the panel's report without a host (ISS-167)", async () => {
    const user = userEvent.setup();
    renderLayout();
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    await user.click(await screen.findByRole("button", { name: "Open child" }));
    const nested = new URLSearchParams(window.location.search);
    expect(nested.get("peekIssueId")).toBe("123e4567-e89b-12d3-a456-426614174099");
    expect(nested.get("peekNestingLevel")).toBe("1");
    await user.click(screen.getByRole("button", { name: "Back to parent" }));
    const back = new URLSearchParams(window.location.search);
    expect(back.get("peekIssueId")).toBe(ISSUE);
    expect(back.get("peekNestingLevel")).toBeNull();
    // The root panel stays mounted while the child holds the address.
    expect(screen.getByLabelText("Work item preview")).toHaveAttribute("data-peeked", ISSUE);
  });

  it("writes a foreign row's project into the address (ISS-167)", async () => {
    const foreign = "823e4567-e89b-12d3-a456-426614174097";
    mocks.pages.mockReturnValue(
      pageState({
        sections: [{ key: "all", title: "All", rows: [row({ project_id: foreign })], total: 1, value: null }],
      })
    );
    const user = userEvent.setup();
    renderLayout();
    await user.click(screen.getByRole("button", { name: "Fix login" }));
    expect(new URLSearchParams(window.location.search).get("peekProjectId")).toBe(foreign);
  });

  it("mounts the resolving peek for a nested row key outside the top-level pages", async () => {
    renderLayout();
    fireEvent.click(screen.getByRole("button", { name: "Nested child" }));
    expect(await screen.findByLabelText("Work item preview")).toHaveAttribute("data-peeked", "WEB-99");
  });

  it("mounts the resolving peek for a key that matches no loaded row", async () => {
    renderLayout({ peekKey: "WEB-404" });
    expect(await screen.findByLabelText("Work item preview")).toHaveAttribute("data-peeked", "WEB-404");
  });

  it("hands the peeked project through to the resolving peek", async () => {
    renderLayout({ peekKey: "issue-9", peekProjectId: "project-9" });
    const panel = await screen.findByLabelText("Work item preview");
    expect(panel).toHaveAttribute("data-peeked", "issue-9");
    expect(panel).toHaveAttribute("data-peek-project", "project-9");
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

  it("closes section creation where the workflow gate refuses it, per section (ISS-233)", async () => {
    const sections = groupedSections(
      { results: { [STATE]: { results: [row()], total_results: 1 }, [DONE_STATE]: { results: [], total_results: 0 } } },
      new Map([
        [STATE, "Todo"],
        [DONE_STATE, "Done"],
      ]),
      { showEmptyGroups: true, dimension: "state" }
    );
    mocks.pages.mockReturnValue(pageState({ sections, total: 1, grouped: true }));
    edition.flags[WORKFLOW_FLAG] = true;
    registerEditionIssuesProviders({
      workflowRules: {
        refusal: () => null,
        creationRefusal: ({ columnKey }) => (columnKey === DONE_STATE ? { reason: "Start items in Todo" } : null),
      },
    });
    renderLayout({ prefs: prefs({ groupBy: "state", showEmptyGroups: true }) });
    const [todoProbe, doneProbe] = await screen.findAllByTestId("quick-add-probe");
    expect(todoProbe).toHaveAttribute("data-disabled", "false");
    expect(doneProbe).toHaveAttribute("data-disabled", "true");
    expect(doneProbe).toHaveAttribute("data-reason", "Start items in Todo");
  });

  it("keeps every section open without the workflow flag (OSS absence)", async () => {
    const sections = groupedSections(
      { results: { [STATE]: { results: [row()], total_results: 1 }, [DONE_STATE]: { results: [], total_results: 0 } } },
      new Map([
        [STATE, "Todo"],
        [DONE_STATE, "Done"],
      ]),
      { showEmptyGroups: true, dimension: "state" }
    );
    mocks.pages.mockReturnValue(pageState({ sections, total: 1, grouped: true }));
    registerEditionIssuesProviders({
      workflowRules: {
        refusal: () => null,
        creationRefusal: () => ({ reason: "no rule should be asked" }),
      },
    });
    renderLayout({ prefs: prefs({ groupBy: "state", showEmptyGroups: true }) });
    const probes = await screen.findAllByTestId("quick-add-probe");
    expect(probes).toHaveLength(2);
    for (const probe of probes) expect(probe).toHaveAttribute("data-disabled", "false");
  });

  it("keeps quick-add enabled for an ordinary project view", async () => {
    renderLayout();
    expect(await screen.findByTestId("quick-add-probe")).toHaveAttribute("data-disabled", "false");
  });

  it("registers the create and add-existing entries for the empty card (ISS-069/070)", async () => {
    const user = userEvent.setup();
    const onRegisterOverlays = vi.fn();
    renderLayout({ cycleId: CYCLE, moduleId: MODULE, onRegisterOverlays });
    await waitFor(() => expect(onRegisterOverlays).toHaveBeenCalledTimes(1));
    const overlays = onRegisterOverlays.mock.calls[0]![0] as {
      openCreate: () => void;
      openAddExisting: () => void;
    };
    act(() => {
      overlays.openCreate();
    });
    const dialog = await screen.findByTestId("create-dialog");
    expect(dialog).toHaveAttribute("data-project", PROJECT);
    expect(JSON.parse(dialog.getAttribute("data-values") ?? "{}")).toEqual({ cycleId: CYCLE, moduleIds: [MODULE] });
    await user.click(screen.getByRole("button", { name: "create" }));
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument());
    act(() => {
      overlays.openAddExisting();
    });
    expect(await screen.findByTestId("add-existing-modal")).toHaveAttribute("data-module", MODULE);
  });

  it("refreshes the entity's lists after attaching from the empty card", async () => {
    const user = userEvent.setup();
    const onRegisterOverlays = vi.fn();
    const { queryClient } = renderLayout({ moduleId: MODULE, onRegisterOverlays });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    await waitFor(() => expect(onRegisterOverlays).toHaveBeenCalledTimes(1));
    act(() => {
      (onRegisterOverlays.mock.calls[0]![0] as { openAddExisting: () => void }).openAddExisting();
    });
    await user.click(await screen.findByRole("button", { name: "Attach one" }));
    await waitFor(() => expect(mocks.addToModule).toHaveBeenCalled());
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: issueKeys.lists("acme", ...entityListsKeyPart({ scope: "project", projectId: PROJECT })),
      })
    );
  });

  it("hands each row its menu context and the viewer's standing (ISS-060/062/063)", () => {
    mocks.pages.mockReturnValue(pageState());
    renderLayout({ cycleId: CYCLE });
    const row = screen.getAllByTestId("row-probe")[0]!;
    expect(row).toHaveAttribute("data-menu-context", "cycle");
    expect(row).toHaveAttribute("data-menu-cycle", CYCLE);
    expect(row).toHaveAttribute("data-menu-write", "true");
  });

  it("opens the project context on a plain list and reads a completed cycle as read-only", () => {
    mocks.pages.mockReturnValue(pageState());
    renderLayout({});
    expect(screen.getAllByTestId("row-probe")[0]).toHaveAttribute("data-menu-context", "project");
    cleanup();
    mocks.pages.mockReturnValue(pageState());
    renderLayout({ moduleId: MODULE, completedModule: true });
    const row = screen.getAllByTestId("row-probe")[0]!;
    expect(row).toHaveAttribute("data-menu-context", "module");
    expect(row).toHaveAttribute("data-menu-module", MODULE);
    expect(row).toHaveAttribute("data-menu-write", "false");
  });
});

describe("ListLayout selection", () => {
  it("selects a row and shows the OSS upgrade banner without a toolbar", async () => {
    renderLayout();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
    expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument();
  });

  it("toggles a group from its tri-state header control", async () => {
    const sections = groupedSections(
      { results: { [STATE]: { results: [row()], total_results: 1 } } },
      new Map([[STATE, "Todo"]]),
      { showEmptyGroups: false, dimension: "state" }
    );
    mocks.pages.mockReturnValue(pageState({ sections, total: 1, grouped: true }));
    renderLayout({ prefs: prefs({ groupBy: "state" }) });
    fireEvent.click(screen.getByRole("checkbox", { name: "Select all work items in Todo" }));
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
    fireEvent.click(screen.getByRole("checkbox", { name: "Select all work items in Todo" }));
    await waitFor(() => expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument());
  });

  it("replaces the banner with the cloud toolbar when the edition enables bulk operations", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    renderLayout();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    expect(await screen.findByTestId("bulk-toolbar")).toHaveTextContent("1 selected");
    expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument();
  });

  it("hands the toolbar the selected rows' states for the workflow gate (ISS-233)", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    renderLayout();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    expect(await screen.findByTestId("bulk-toolbar")).toHaveAttribute("data-from-states", JSON.stringify([STATE]));
  });

  it.each([
    ["Update", bulkMocks.update, { issueIds: [ISSUE], properties: { priority: "high" } }],
    ["Delete", bulkMocks.remove, [ISSUE]],
    ["Archive", bulkMocks.archive, [ISSUE]],
    ["Subscribe", bulkMocks.subscribe, [ISSUE]],
  ] as const)("runs the cloud %s action and clears on success", async (label, action, expected) => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    renderLayout();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    fireEvent.click(within(await screen.findByTestId("bulk-toolbar")).getByRole("button", { name: label }));
    await waitFor(() => expect(action).toHaveBeenCalledWith(expected));
    await waitFor(() => expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument());
  });

  it("keeps the selection when a cloud action fails", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    bulkMocks.remove.mockRejectedValueOnce(new Error("refused"));
    renderLayout();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => expect(bulkMocks.remove).toHaveBeenCalled());
    expect(screen.getByTestId("bulk-toolbar")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument();
  });

  it("keeps foreign-project rows out of the selection range", async () => {
    const FOREIGN = "923e4567-e89b-12d3-a456-426614174009";
    mocks.pages.mockReturnValue(
      pageState({
        sections: [
          {
            key: "all",
            title: "All work items",
            rows: [row(), row({ id: FOREIGN, sequence_id: 77, project_id: "823e4567-e89b-12d3-a456-426614174008" })],
            total: 2,
            value: null,
          },
        ],
        total: 2,
      })
    );
    renderLayout();
    expect(screen.getByRole("img", { name: "Cannot select 77" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
  });

  it("keeps the cloud toolbar hidden for readers without edit permission", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    mocks.permissions = { canEdit: false };
    renderLayout();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 42" }));
    await waitFor(() => expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument());
    expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument();
  });

  it("adds expanded nested rows to the flattened selection order", async () => {
    renderLayout();
    fireEvent.click(screen.getByRole("button", { name: "Expand selection child" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Select 43" }));
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
    fireEvent.click(screen.getByRole("button", { name: "Collapse selection child" }));
    await waitFor(() => expect(screen.queryByText(/work item selected/)).not.toBeInTheDocument());
  });

  it("reveals an active row beyond the virtualized window", async () => {
    const rows = Array.from({ length: 30 }, (_, index) =>
      row({
        id: `123e4567-e89b-12d3-a456-${String(426614174000 + index)}`,
        sequence_id: index + 1,
        name: `Row ${index + 1}`,
      })
    );
    mocks.pages.mockReturnValue(
      pageState({ sections: [{ key: "all", title: "All work items", rows, total: rows.length, value: null }] })
    );
    renderLayout();
    const boxes = await screen.findAllByRole("checkbox", { name: /^Select \d+$/ });
    const lastRendered = boxes.at(-1)!;
    const lastName = lastRendered.getAttribute("aria-label")!;
    const lastSequence = Number(lastName.replace("Select ", ""));
    const nextSequence = lastSequence + 1;
    fireEvent.click(lastRendered);
    expect(screen.queryByRole("checkbox", { name: `Select ${nextSequence}` })).not.toBeInTheDocument();
    const region = screen.getByRole("region", { name: "Issues" });
    Object.defineProperty(region, "scrollHeight", { configurable: true, value: rows.length * 44 });
    Object.defineProperty(region, "clientHeight", { configurable: true, value: 400 });
    Object.defineProperty(region, "scrollTo", {
      configurable: true,
      value: ({ top }: { top: number }) => {
        region.scrollTop = top;
        fireEvent.scroll(region);
      },
    });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(await screen.findByRole("checkbox", { name: `Select ${nextSequence}` })).toBeInTheDocument();
    expect(region.scrollTop).toBeGreaterThan(0);
  });

  it("ignores repeated expand and collapse reports and waits for child rows", async () => {
    renderLayout();
    fireEvent.click(screen.getByRole("button", { name: "Expand without children" }));
    fireEvent.click(screen.getByRole("button", { name: "Expand without children" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse without children" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse without children" }));
    expect(screen.getByTestId("row-probe")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Select 43" })).not.toBeInTheDocument();
  });

  it("resolves nested row keys by project and falls back for an unknown project", async () => {
    const { queryClient } = renderLayout();
    expect(screen.getByTestId("row-probe")).toHaveAttribute("data-own-key", "WEB");
    expect(screen.getByTestId("row-probe")).toHaveAttribute("data-foreign-key", "WEB");
    const projects = queryClient.getQueryData(projectKeys.list("acme")) as Array<Record<string, unknown>>;
    act(() => {
      queryClient.setQueryData(projectKeys.list("acme"), [
        ...projects,
        {
          ...projects[0]!,
          id: "823e4567-e89b-12d3-a456-426614174008",
          name: "Other",
          identifier: "OTH",
        },
      ]);
    });
    await waitFor(() => expect(screen.getByTestId("row-probe")).toHaveAttribute("data-foreign-key", "OTH"));
  });

  it("keeps the child map stable when the same children are reported again", async () => {
    renderLayout();
    fireEvent.click(screen.getByRole("button", { name: "Expand without children" }));
    fireEvent.click(screen.getByRole("button", { name: "Report stable children" }));
    expect(await screen.findByRole("checkbox", { name: "Select 44" })).toBeInTheDocument();
  });

  it("disables row and group controls when the list opts out of selection", async () => {
    const sections = groupedSections(
      { results: { [STATE]: { results: [row()], total_results: 1 } } },
      new Map([[STATE, "Todo"]]),
      { showEmptyGroups: false, dimension: "state" }
    );
    mocks.pages.mockReturnValue(pageState({ sections, total: 1, grouped: true }));
    renderLayout({ enableSelection: false, prefs: prefs({ groupBy: "state" }) });
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /Cannot select/ })).not.toBeInTheDocument();
  });
});

describe("ListLayout fresh bypass (ISS-006)", () => {
  function freshRow(overrides: Partial<IssueListItem> = {}): IssueListItem {
    const now = new Date().toISOString();
    return row({ created_at: now, updated_at: now, ...overrides });
  }

  function flatSections(rows: IssueListItem[]) {
    return [{ key: "all", title: "All work items", rows, total: rows.length, value: null }];
  }

  function staleRows(count: number, firstSequence = 1): IssueListItem[] {
    return Array.from({ length: count }, (_, index) =>
      row({
        id: `123e4567-e89b-12d3-a456-42661417${String(5000 + firstSequence + index)}`,
        sequence_id: firstSequence + index,
        name: `Row ${firstSequence + index}`,
      })
    );
  }

  it("renders fresh rows in a strip above the virtualized list", async () => {
    const rows = [
      ...staleRows(2),
      freshRow({
        id: "123e4567-e89b-12d3-a456-426614176000",
        sequence_id: 3,
        name: "Fresh confirmed",
      }),
    ];
    mocks.pages.mockReturnValue(pageState({ sections: flatSections(rows), total: rows.length }));
    renderLayout();

    const strip = await screen.findByTestId("issues-fresh-strip");
    expect(within(strip).getByRole("button", { name: "Fresh confirmed" })).toBeInTheDocument();
    expect(within(strip).queryByRole("button", { name: "Row 1" })).not.toBeInTheDocument();
    // The bypass moves the row out of the virtualizer instead of copying it.
    expect(screen.getAllByRole("button", { name: "Fresh confirmed" })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Row 1" })).toBeInTheDocument();
    const region = screen.getByRole("region", { name: "Issues" });
    expect(strip.compareDocumentPosition(region) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders the pending temp row exactly once", async () => {
    const temp = freshRow({
      id: "temp-9f8e7d6c-test-row",
      sequence_id: 0,
      project_id: "",
      name: "Saving row",
    });
    const rows = [temp, ...staleRows(1)];
    mocks.pages.mockReturnValue(pageState({ sections: flatSections(rows), total: rows.length }));
    renderLayout();

    const strip = await screen.findByTestId("issues-fresh-strip");
    expect(within(strip).getByRole("button", { name: "Saving row" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Saving row" })).toHaveLength(1);
  });

  it("shows only the strip while the first create pends", async () => {
    const temp = freshRow({
      id: "temp-first-create-row",
      sequence_id: 0,
      project_id: "",
      name: "First row",
    });
    mocks.pages.mockReturnValue(pageState({ sections: flatSections([temp]), total: 1 }));
    renderLayout();

    const strip = await screen.findByTestId("issues-fresh-strip");
    expect(within(strip).getByRole("button", { name: "First row" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "First row" })).toHaveLength(1);
  });

  it("omits the strip when no row is fresh", async () => {
    renderLayout();

    expect(await screen.findByTestId("row-probe")).toBeInTheDocument();
    expect(screen.queryByTestId("issues-fresh-strip")).not.toBeInTheDocument();
  });

  it("keeps grouped sections unpartitioned", async () => {
    const fresh = freshRow({ sequence_id: 7, name: "Fresh grouped" });
    mocks.pages.mockReturnValue(
      pageState({
        grouped: true,
        sections: [{ key: STATE, title: "Todo", rows: [fresh], total: 1, value: STATE }],
        total: 1,
      })
    );
    renderLayout({ prefs: prefs({ groupBy: "state" }) });

    expect(await screen.findByRole("button", { name: "Fresh grouped" })).toBeInTheDocument();
    expect(screen.queryByTestId("issues-fresh-strip")).not.toBeInTheDocument();
  });

  it("renders no strip for a flat list with no sections", () => {
    mocks.pages.mockReturnValue(pageState({ sections: [], total: 0 }));
    renderLayout();

    expect(screen.queryByTestId("issues-fresh-strip")).not.toBeInTheDocument();
    expect(screen.queryByTestId("row-probe")).not.toBeInTheDocument();
  });

  it("skips the scroll reveal for bypassed rows", async () => {
    const rows = [
      ...staleRows(12),
      freshRow({
        id: "123e4567-e89b-12d3-a456-426614176001",
        sequence_id: 13,
        name: "Fresh tail",
      }),
    ];
    mocks.pages.mockReturnValue(pageState({ sections: flatSections(rows), total: rows.length }));
    renderLayout();

    expect(await screen.findByRole("button", { name: "Fresh tail" })).toBeInTheDocument();
    const region = screen.getByRole("region", { name: "Issues" });
    Object.defineProperty(region, "scrollHeight", { configurable: true, value: 12 * 44 });
    Object.defineProperty(region, "clientHeight", { configurable: true, value: 400 });
    const scrollTo = vi.fn();
    Object.defineProperty(region, "scrollTo", { configurable: true, value: scrollTo });
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 12" }));
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    // The fresh tail is already visible above the virtualizer: no scroll.
    expect(scrollTo).not.toHaveBeenCalled();
    // And the way back still reveals through the virtualizer.
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp" }));
    });
    await waitFor(() => expect(scrollTo).toHaveBeenCalled());
  });

  it("reveals virtualized rows past a bypassed temp row", async () => {
    const temp = freshRow({
      id: "temp-9f8e7d6c-reveal-row",
      sequence_id: 0,
      project_id: "",
      name: "Saving row",
    });
    const rows = [temp, ...staleRows(30)];
    mocks.pages.mockReturnValue(pageState({ sections: flatSections(rows), total: rows.length }));
    renderLayout();

    const boxes = await screen.findAllByRole("checkbox", { name: /^Select \d+$/ });
    const lastRendered = boxes.at(-1)!;
    const lastSequence = Number(lastRendered.getAttribute("aria-label")!.replace("Select ", ""));
    const nextSequence = lastSequence + 1;
    fireEvent.click(lastRendered);
    expect(screen.queryByRole("checkbox", { name: `Select ${nextSequence}` })).not.toBeInTheDocument();
    const region = screen.getByRole("region", { name: "Issues" });
    Object.defineProperty(region, "scrollHeight", { configurable: true, value: rows.length * 44 });
    Object.defineProperty(region, "clientHeight", { configurable: true, value: 400 });
    Object.defineProperty(region, "scrollTo", {
      configurable: true,
      value: ({ top }: { top: number }) => {
        region.scrollTop = top;
        fireEvent.scroll(region);
      },
    });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(await screen.findByRole("checkbox", { name: `Select ${nextSequence}` })).toBeInTheDocument();
    expect(region.scrollTop).toBeGreaterThan(0);
  });

  it("skips the scroll reveal once the flat list unmounts", () => {
    renderLayout({}, { kanban_filters: { group_by: ["all"], sub_group_by: [] } });
    expect(screen.queryByTestId("row-probe")).not.toBeInTheDocument();

    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    expect(screen.queryByTestId("row-probe")).not.toBeInTheDocument();
  });

  it("caps the strip with a hundred fresh rows so the region still pages (ISS-010, NEWFRONT-1014)", async () => {
    const fresh = Array.from({ length: 100 }, (_, index) =>
      freshRow({
        id: `123e4567-e89b-12d3-a456-42661417${String(6100 + index)}`,
        sequence_id: 1000 + index,
        name: `Fresh ${index + 1}`,
      })
    );
    const rows = [...staleRows(3), ...fresh];
    mocks.pages.mockReturnValue(pageState({ sections: flatSections(rows), total: rows.length, hasMore: true }));
    renderLayout();

    // The strip holds the first rows only: the rest virtualize below, so a
    // screenful of fresh rows cannot squeeze the region to zero height.
    const overflowName = `Fresh ${FRESH_BYPASS_LIMIT + 1}`;
    const strip = await screen.findByTestId("issues-fresh-strip");
    expect(within(strip).getAllByTestId("row-probe")).toHaveLength(FRESH_BYPASS_LIMIT);
    expect(within(strip).getByRole("button", { name: "Fresh 1" })).toBeInTheDocument();
    expect(within(strip).queryByRole("button", { name: overflowName })).not.toBeInTheDocument();

    const region = screen.getByRole("region", { name: "Issues" });
    expect(within(region).getByRole("button", { name: "Row 1" })).toBeInTheDocument();
    expect(within(region).getByRole("button", { name: overflowName })).toBeInTheDocument();
    // Bypassed or virtualized, never both.
    expect(screen.getAllByRole("button", { name: overflowName })).toHaveLength(1);
    expect(screen.getByTestId("list-sentinel")).toBeInTheDocument();
  });

  it("loads the next page when the sentinel intersects past a capped strip (ISS-010, NEWFRONT-1014)", async () => {
    const seen: Array<{ callback: IntersectionObserverCallback; target: Element | null }> = [];
    class CapturingObserver {
      constructor(callback: IntersectionObserverCallback) {
        seen.push({ callback, target: null });
      }
      observe(target: Element): void {
        seen[seen.length - 1]!.target = target;
      }
      disconnect(): void {}
    }
    vi.stubGlobal("IntersectionObserver", CapturingObserver);
    const loadMore = vi.fn();
    const fresh = Array.from({ length: 100 }, (_, index) =>
      freshRow({
        id: `123e4567-e89b-12d3-a456-42661417${String(6200 + index)}`,
        sequence_id: 2000 + index,
        name: `Paged ${index + 1}`,
      })
    );
    const rows = [...staleRows(3), ...fresh];
    mocks.pages.mockReturnValue(
      pageState({ sections: flatSections(rows), total: rows.length, hasMore: true, loadMore })
    );
    renderLayout();

    const sentinel = await screen.findByTestId("list-sentinel");
    expect(seen).toHaveLength(1);
    expect(seen[0]!.target).toBe(sentinel);
    seen[0]!.callback([{ isIntersecting: true } as IntersectionObserverEntry], null as unknown as IntersectionObserver);
    expect(loadMore).toHaveBeenCalledTimes(1);
  });
});
