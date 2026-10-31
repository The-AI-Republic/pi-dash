// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view detail screen (parity/specs/views.md §11, VIEW-038/040).
// The embedded surfaces own their internals (issues suites for the list,
// the filter row and Display; views suites for the dialogs, menus and tab
// strip); this suite pins what the detail owns: default versus custom
// resolution, the list-membership access check, the header controls per
// lock and role, the body query scoping, the missing state, and the
// cloud-only layout selector with its display-filter persist.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-ins for the router: navigation is TanStack's own tested behavior.
const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

// The session stands in for the gated layout's reads: role, identity and
// the workspace name behind the tab title.
const sessionHarness = vi.hoisted(() => ({
  role: 15,
  meId: "123e4567-e89b-12d3-a456-426614174000",
  mePending: false,
  meFailed: false,
  meRefetch: vi.fn(),
  workspaceName: "Acme",
}));
vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  usePermissions: () => ({
    role: sessionHarness.role,
    isMember: true,
    canEdit: sessionHarness.role >= 15,
    isAdmin: sessionHarness.role >= 20,
    isLoading: false,
  }),
  useMe: () => ({
    data: sessionHarness.mePending || sessionHarness.meFailed ? undefined : { id: sessionHarness.meId },
    isPending: sessionHarness.mePending,
    isError: sessionHarness.meFailed,
    refetch: sessionHarness.meRefetch,
  }),
  useWorkspace: () => ({
    workspace:
      sessionHarness.workspaceName === null
        ? undefined
        : { name: sessionHarness.workspaceName, slug: "acme", role: sessionHarness.role },
  }),
}));

// The edition decides the layout selector; the harness swaps in a cloud
// object for the VIEW-040 branch.
const editionHarness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return editionHarness.edition;
  },
}));

// Failure notices are captured; the tab title stays real.
const shellHarness = vi.hoisted(() => ({ notifyError: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/index.js")>()),
  notifyError: shellHarness.notifyError,
}));

const listHarness = vi.hoisted(() => ({
  query: null as unknown,
  hideHeader: false,
  hasEmptyState: false,
  label: "",
  peekKey: null as string | null | undefined,
  peekProjectId: null as string | null | undefined,
  onPeekChange: null as ((request: unknown) => void) | null,
}));
vi.mock("../../issues/global/GlobalIssuesScreen.js", () => ({
  GlobalIssuesScreen: (props: {
    query?: unknown;
    hideHeader?: boolean;
    emptyState?: React.ReactNode;
    label?: string;
    peekKey?: string | null;
    peekProjectId?: string | null;
    onPeekChange?: (request: unknown) => void;
  }) => {
    listHarness.query = props.query ?? null;
    listHarness.hideHeader = props.hideHeader ?? false;
    listHarness.hasEmptyState = props.emptyState !== undefined && props.emptyState !== null;
    listHarness.label = props.label ?? "";
    listHarness.peekKey = props.peekKey;
    listHarness.peekProjectId = props.peekProjectId;
    listHarness.onPeekChange = props.onPeekChange ?? null;
    return (
      <div data-testid="global-list" data-label={props.label ?? ""}>
        {props.emptyState}
      </div>
    );
  },
}));

const filtersHarness = vi.hoisted(() => ({
  props: null as {
    view?: { id: string; name: string };
    viewDisplay?: unknown;
    onViewUpdated?: (view: never) => void;
    onViewCreated?: (view: never) => void;
  } | null,
}));
vi.mock("../../issues/filters/rich/lazy.js", () => ({
  LazyIssueFilters: (props: {
    children: (parts: { control: React.ReactNode; row: React.ReactNode }) => React.ReactNode;
    view?: { id: string; name: string };
    viewDisplay?: unknown;
    onViewUpdated?: (view: never) => void;
    onViewCreated?: (view: never) => void;
  }) => {
    filtersHarness.props = props;
    return (
      <>
        {props.children({
          control: <span data-testid="row-control" />,
          row: <span data-testid="filter-row" />,
        })}
      </>
    );
  },
}));

const defaultFiltersHarness = vi.hoisted(() => ({
  onExpressionChange: null as ((expression: Record<string, unknown>) => void) | null,
}));
vi.mock("./WorkspaceDefaultFilters.js", () => ({
  WorkspaceDefaultFilters: (props: {
    children: (slots: { control: React.ReactNode; row: React.ReactNode }) => React.ReactNode;
    onExpressionChange: (expression: Record<string, unknown>) => void;
  }) => {
    defaultFiltersHarness.onExpressionChange = props.onExpressionChange;
    return (
      <>
        {props.children({
          control: <span data-testid="default-filter-control" />,
          row: <span data-testid="default-filter-row" />,
        })}
      </>
    );
  },
}));

const displayHarness = vi.hoisted(() => ({
  props: null as { display?: unknown; onDisplayChange?: (next: unknown) => void; offered?: readonly unknown[] } | null,
}));
vi.mock("../../issues/display/DisplayControl.js", () => ({
  DisplayControl: (props: {
    display?: unknown;
    onDisplayChange?: (next: unknown) => void;
    offered?: readonly unknown[];
  }) => {
    displayHarness.props = props;
    return <span data-testid="display-control" data-mode={props.display === undefined ? "entity" : "saved"} />;
  },
}));

const switcherHarness = vi.hoisted(() => ({
  offered: [] as string[],
  active: "",
}));
vi.mock("../../issues/components/LayoutSwitcher.js", () => ({
  LayoutSwitcher: (props: { offered: string[]; active: string; onSwitch: (kind: string) => void }) => {
    switcherHarness.offered = props.offered;
    switcherHarness.active = props.active;
    return (
      <div data-testid="layout-switcher" data-active={props.active}>
        {props.offered.map((kind) => (
          <button key={kind} type="button" onClick={() => props.onSwitch(kind)}>
            {`switch-${kind}`}
          </button>
        ))}
      </div>
    );
  },
}));

const prefsHarness = vi.hoisted(() => ({
  record: undefined as
    | { display_filters: Record<string, unknown>; display_properties: Record<string, unknown> }
    | undefined,
  updates: [] as Array<Record<string, unknown>>,
}));
vi.mock("../../issues/prefs/displayPrefs.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../issues/prefs/displayPrefs.js")>()),
  useEntityPrefs: () => ({ data: prefsHarness.record }),
  useUpdateDisplayPrefs: () => ({
    updateDisplayPrefs: (partial: Record<string, unknown>) => {
      prefsHarness.updates.push(partial);
    },
  }),
}));

vi.mock("../../issues/emptyStates/viewEmpty.js", () => ({
  SavedViewEmptyCard: (props: { canAct: boolean; onCreate: () => void }) => (
    <div data-testid="empty-card" data-can-act={String(props.canAct)}>
      <button type="button" onClick={props.onCreate}>
        New work item
      </button>
    </div>
  ),
  loadSavedViewEmptyCard: () => Promise.resolve(),
}));
vi.mock("../../issues/components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    projects: Array<{ id: string }>;
  }) => (
    <div data-testid="create-dialog" data-open={String(props.open)} data-projects={props.projects.length}>
      <button type="button" onClick={() => props.onOpenChange(false)}>
        close-create
      </button>
    </div>
  ),
}));

const stripHarness = vi.hoisted(() => ({ activeViewId: null as string | null }));
vi.mock("../components/WorkspaceViewsTabStrip.js", () => ({
  WorkspaceViewsTabStrip: (props: { activeViewId: string | null }) => {
    stripHarness.activeViewId = props.activeViewId;
    return <div data-testid="tab-strip" />;
  },
}));

vi.mock("../dialog/WorkspaceViewDialog.js", () => ({
  WorkspaceViewDialog: (props: { mode: string; view?: { id: string }; onOpenChange: (open: boolean) => void }) => (
    <div data-testid="view-dialog" data-mode={props.mode} data-view={props.view?.id ?? ""}>
      <button type="button" onClick={() => props.onOpenChange(false)}>
        close-view-dialog
      </button>
    </div>
  ),
}));
vi.mock("../dialog/DeleteWorkspaceViewDialog.js", () => ({
  DeleteWorkspaceViewDialog: (props: { view: { id: string }; onDeleted?: (viewId: string) => void }) => (
    <div data-testid="delete-dialog">
      <button type="button" onClick={() => props.onDeleted?.(props.view.id)}>
        confirm-delete
      </button>
    </div>
  ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { VIEWS_OWNER_ID, jsonResponse, makeView } from "../testing.js";
import { WorkspaceViewScreen, type WorkspacePeekAddress } from "./WorkspaceViewScreen.js";

const PEEKED_ISSUE = "723e4567-e89b-12d3-a456-426614174011";
const PEEKED_PROJECT = "823e4567-e89b-12d3-a456-426614174012";

/** The address the route would hand the screen; tests record its writer. */
const peekHarness: { requests: unknown[] } = { requests: [] };
function peekAddress(overrides: Partial<WorkspacePeekAddress> = {}): WorkspacePeekAddress {
  return {
    peekKey: null,
    peekProjectId: null,
    onPeekChange: (request) => peekHarness.requests.push(request),
    ...overrides,
  };
}

const CUSTOM_A = "223e4567-e89b-12d3-a456-426614174001";
const CUSTOM_B = "323e4567-e89b-12d3-a456-426614174002";

interface Harness {
  views: ReturnType<typeof makeView>[];
  viewsStatus: number;
  view: Record<string, unknown> | null;
  viewStatus: number;
  retrieveCalls: number;
  patches: Array<Record<string, unknown>>;
  patchStatus: number;
}

let harness: Harness;

function projectRow(overrides: Record<string, unknown> = {}): Project & Record<string, unknown> {
  return {
    id: "423e4567-e89b-12d3-a456-426614174003",
    name: "Website",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  } as Project & Record<string, unknown>;
}

beforeEach(() => {
  routerHarness.navigate.mockReset();
  shellHarness.notifyError.mockReset();
  sessionHarness.role = 15;
  sessionHarness.meId = VIEWS_OWNER_ID;
  sessionHarness.mePending = false;
  sessionHarness.meFailed = false;
  sessionHarness.meRefetch.mockReset();
  sessionHarness.workspaceName = "Acme";
  editionHarness.edition = { id: "oss", flags: {} };
  listHarness.query = null;
  listHarness.hideHeader = false;
  listHarness.hasEmptyState = false;
  listHarness.label = "";
  listHarness.peekKey = undefined;
  listHarness.peekProjectId = undefined;
  listHarness.onPeekChange = null;
  peekHarness.requests = [];
  filtersHarness.props = null;
  defaultFiltersHarness.onExpressionChange = null;
  displayHarness.props = null;
  switcherHarness.offered = [];
  switcherHarness.active = "";
  prefsHarness.record = undefined;
  prefsHarness.updates = [];
  stripHarness.activeViewId = null;
  harness = {
    views: [
      makeView({ id: CUSTOM_A, project: null, name: "Triage", owned_by: VIEWS_OWNER_ID }),
      makeView({ id: CUSTOM_B, project: null, name: "Later", owned_by: VIEWS_OWNER_ID }),
    ],
    viewsStatus: 200,
    view: makeView({
      id: CUSTOM_A,
      project: null,
      name: "Triage",
      owned_by: VIEWS_OWNER_ID,
      display_filters: { layout: "spreadsheet", order_by: "-created_at" },
      display_properties: {},
      rich_filters: { priority: "urgent" },
    }),
    viewStatus: 200,
    retrieveCalls: 0,
    patches: [],
    patchStatus: 200,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.includes("/api/workspaces/acme/views/") && init.method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      harness.patches.push(body);
      if (harness.patchStatus !== 200) return jsonResponse(url, harness.patchStatus, {});
      return jsonResponse(url, 200, {
        ...(harness.view ?? {}),
        display_filters: {
          ...((harness.view?.["display_filters"] as Record<string, unknown>) ?? {}),
          ...(body["display_filters"] as Record<string, unknown>),
        },
      });
    }
    if (
      url.includes(`/api/workspaces/acme/views/${CUSTOM_A}/`) ||
      url.includes(`/api/workspaces/acme/views/${CUSTOM_B}/`)
    ) {
      harness.retrieveCalls += 1;
      if (harness.view === null) return jsonResponse(url, harness.viewStatus, {});
      return jsonResponse(url, harness.viewStatus, harness.view);
    }
    if (url.split("?")[0]?.endsWith("/api/workspaces/acme/views/")) {
      return jsonResponse(url, harness.viewsStatus, harness.views);
    }
    if (url.split("?")[0]?.endsWith("/api/workspaces/acme/projects/")) {
      return jsonResponse(url, 200, [
        projectRow(),
        projectRow({
          id: "523e4567-e89b-12d3-a456-426614174004",
          name: "Old",
          identifier: "OLD",
          archived_at: "2026-01-01T00:00:00Z",
        }),
      ]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
});

function renderScreen(viewId: string, peek: WorkspacePeekAddress = peekAddress()): { queryClient: QueryClient } {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceViewScreen params={{ ws: "acme", viewId }} peek={peek} />
    </QueryClientProvider>
  );
  return { queryClient };
}

describe("WorkspaceViewScreen defaults", () => {
  it("renders the static default with crumbs, switcher, controls, strip and scoped body", async () => {
    renderScreen("assigned");
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    await waitFor(() => expect(document.title).toBe("Acme - All Views"));
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(trail).getByRole("link", { name: "Views" })).toHaveAttribute("href", "/acme/workspace-views");
    expect(within(trail).getByRole("button", { name: "Assigned" })).toBeInTheDocument();
    expect(screen.getByTestId("tab-strip")).toBeInTheDocument();
    expect(stripHarness.activeViewId).toBe("assigned");
    expect(screen.getByTestId("default-filter-control")).toBeInTheDocument();
    expect(screen.getByTestId("default-filter-row")).toBeInTheDocument();
    expect(screen.getByTestId("display-control")).toHaveAttribute("data-mode", "entity");
    // OSS renders no layout selector.
    expect(screen.queryByTestId("layout-switcher")).toBeNull();
    // The body read carries the static condition with the surface order.
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
    expect(listHarness.query).toMatchObject({ orderBy: "-created_at", filters: { assignees: VIEWS_OWNER_ID } });
    expect(listHarness.hideHeader).toBe(true);
    expect(listHarness.label).toBe("Assigned");
    expect(listHarness.hasEmptyState).toBe(true);
    expect(harness.retrieveCalls).toBe(0);
  });

  it("reads all-issues plain with the workspace empty card", async () => {
    renderScreen("all-issues");
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
    expect(listHarness.query).toEqual({ orderBy: "-created_at" });
    expect(listHarness.label).toBe("All work items");
    expect(listHarness.hasEmptyState).toBe(false);
  });

  it("follows the row's live expression on top of the static condition", async () => {
    renderScreen("assigned");
    await screen.findByTestId("global-list");
    defaultFiltersHarness.onExpressionChange?.({ priority: "urgent" });
    await waitFor(() =>
      expect(listHarness.query).toMatchObject({
        filters: { assignees: VIEWS_OWNER_ID, filters: '{"priority":"urgent"}' },
      })
    );
  });

  it("follows the stored personal order", async () => {
    prefsHarness.record = { display_filters: { order_by: "priority" }, display_properties: {} };
    renderScreen("all-issues");
    await screen.findByTestId("global-list");
    expect(listHarness.query).toMatchObject({ orderBy: "priority" });
  });

  it("renders while the customs list loads and reports its failure inline", async () => {
    harness.viewsStatus = 500;
    renderScreen("assigned");
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Assigned" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Views could not be loaded."));
  });

  it("waits for the session on member-scoped defaults and retries its failure", async () => {
    sessionHarness.mePending = true;
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <WorkspaceViewScreen params={{ ws: "acme", viewId: "assigned" }} peek={peekAddress()} />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("issues-skeleton-list")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-view-header")).toBeNull();
    unmount();
    sessionHarness.mePending = false;
    sessionHarness.meFailed = true;
    renderScreen("assigned");
    fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
    expect(sessionHarness.meRefetch).toHaveBeenCalled();
  });

  it("adds a view from a default as a member", async () => {
    renderScreen("assigned");
    fireEvent.click(await screen.findByTestId("workspace-view-add"));
    expect(await screen.findByTestId("view-dialog")).toHaveAttribute("data-mode", "create");
  });

  it("hides the add action from guests but still reads the body", async () => {
    sessionHarness.role = 5;
    renderScreen("all-issues");
    await screen.findByTestId("workspace-view-header");
    expect(screen.queryByTestId("workspace-view-add")).toBeNull();
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
  });

  it("opens the work item dialog from the default's empty card", async () => {
    renderScreen("assigned");
    fireEvent.click(await screen.findByRole("button", { name: "New work item" }));
    expect(await screen.findByTestId("create-dialog")).toHaveAttribute("data-open", "true");
  });

  it("switches to another view from the default switcher", async () => {
    renderScreen("assigned");
    fireEvent.click(await screen.findByRole("button", { name: "Assigned" }));
    fireEvent.click(await screen.findByRole("option", { name: "Triage" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/workspace-views/${CUSTOM_A}` });
  });
  it("reads all-issues plain while the session loads", async () => {
    sessionHarness.mePending = true;
    renderScreen("all-issues");
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
    expect(listHarness.query).toEqual({ orderBy: "-created_at" });
  });

  it("opens a stored personal layout on a default", async () => {
    editionHarness.edition = { id: "cloud", flags: {}, slots: { workspaceViewLayouts: { layouts: ["list"] } } };
    prefsHarness.record = { display_filters: { layout: "list" }, display_properties: {} };
    renderScreen("assigned");
    expect(await screen.findByTestId("layout-switcher")).toHaveAttribute("data-active", "list");
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
  });

  it("renders nothing on a default for a stored layout the surface cannot draw", async () => {
    prefsHarness.record = { display_filters: { layout: "kanban" }, display_properties: {} };
    renderScreen("assigned");
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    expect(await screen.findByTestId("workspace-view-empty-body")).toBeInTheDocument();
    expect(screen.queryByTestId("global-list")).toBeNull();
  });

  it("maps the workspace projects into the default's create dialog", async () => {
    renderScreen("assigned");
    fireEvent.click(await screen.findByRole("button", { name: "New work item" }));
    expect(await screen.findByTestId("create-dialog")).toHaveAttribute("data-projects", "1");
  });

  it("hands the route's peek address to the default's body (ISS-167)", async () => {
    renderScreen("all-issues", peekAddress({ peekKey: PEEKED_ISSUE, peekProjectId: PEEKED_PROJECT }));
    await screen.findByTestId("global-list");
    expect(listHarness.peekKey).toBe(PEEKED_ISSUE);
    expect(listHarness.peekProjectId).toBe(PEEKED_PROJECT);
    // A report from the body reaches the route's writer unchanged.
    const request = { issueId: "923e4567-e89b-12d3-a456-426614174013", projectId: PEEKED_PROJECT, nestingLevel: 1 };
    listHarness.onPeekChange?.(request);
    expect(peekHarness.requests).toEqual([request]);
  });
});

describe("WorkspaceViewScreen customs", () => {
  it("renders the custom with its stored expression, order and saved display", async () => {
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    expect(
      within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByRole("button", { name: "Triage" })
    ).toBeInTheDocument();
    expect(stripHarness.activeViewId).toBe(CUSTOM_A);
    expect(screen.getByTestId("row-control")).toBeInTheDocument();
    expect(screen.getByTestId("filter-row")).toBeInTheDocument();
    expect(screen.getByTestId("display-control")).toHaveAttribute("data-mode", "saved");
    expect(filtersHarness.props?.view?.id).toBe(CUSTOM_A);
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
    expect(listHarness.query).toMatchObject({
      orderBy: "-created_at",
      filters: { filters: '{"priority":"urgent"}' },
    });
    expect(listHarness.label).toBe("Triage");
    expect(displayHarness.props?.offered).toEqual(["spreadsheet"]);
  });

  it("hands the route's peek address to the custom's body (ISS-167)", async () => {
    renderScreen(CUSTOM_A, peekAddress({ peekKey: PEEKED_ISSUE, peekProjectId: PEEKED_PROJECT }));
    await screen.findByTestId("global-list");
    expect(listHarness.peekKey).toBe(PEEKED_ISSUE);
    expect(listHarness.peekProjectId).toBe(PEEKED_PROJECT);
    listHarness.onPeekChange?.(null);
    expect(peekHarness.requests).toEqual([null]);
  });

  it("never retrieves an id the visible list hides", async () => {
    renderScreen("523e4567-e89b-12d3-a456-426614174006");
    expect(await screen.findByRole("heading", { name: /View does not exist/ })).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-view-header")).toBeNull();
    expect(screen.queryByTestId("tab-strip")).toBeNull();
    expect(harness.retrieveCalls).toBe(0);
  });

  it.each([403, 404, 500])("renders the missing state for a %s retrieve", async (status) => {
    harness.viewStatus = status;
    harness.view = null;
    renderScreen(CUSTOM_A);
    expect(await screen.findByRole("heading", { name: /View does not exist/ })).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-view-header")).toBeNull();
  });

  it("treats a blank serialized record as missing", async () => {
    harness.view = { ...makeView(), id: "", name: "" };
    renderScreen(CUSTOM_A);
    expect(await screen.findByRole("heading", { name: /View does not exist/ })).toBeInTheDocument();
  });

  it("returns to all-issues from the missing state", async () => {
    harness.views = [];
    renderScreen(CUSTOM_A);
    fireEvent.click(await screen.findByRole("button", { name: "Go to All work items" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: "/acme/workspace-views/all-issues" });
  });

  it("reports a customs failure with a retry instead of guessing access", async () => {
    harness.viewsStatus = 500;
    renderScreen(CUSTOM_A);
    fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
    await waitFor(() => expect(harness.retrieveCalls).toBe(0));
    expect(screen.queryByTestId("workspace-view-header")).toBeNull();
  });

  it("hides selector, toggle, display and row on a locked custom but keeps add and menu", async () => {
    harness.view = { ...makeView({ id: CUSTOM_A, project: null, name: "Triage" }), is_locked: true };
    harness.views = [makeView({ id: CUSTOM_A, project: null, name: "Triage", is_locked: true })];
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    expect(screen.queryByTestId("layout-switcher")).toBeNull();
    expect(screen.queryByTestId("row-control")).toBeNull();
    expect(screen.queryByTestId("filter-row")).toBeNull();
    expect(screen.queryByTestId("display-control")).toBeNull();
    expect(screen.getByTestId("workspace-view-add")).toBeInTheDocument();
    expect(screen.getByTestId("workspace-view-menu-trigger")).toBeInTheDocument();
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
  });

  it("edits and deletes from the owner menu and leaves on delete", async () => {
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("workspace-view-menu-trigger"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }));
    const dialog = await screen.findByTestId("view-dialog");
    expect(dialog).toHaveAttribute("data-mode", "edit");
    expect(dialog).toHaveAttribute("data-view", CUSTOM_A);
    fireEvent.click(screen.getByRole("button", { name: "close-view-dialog" }));
    fireEvent.click(screen.getByTestId("workspace-view-menu-trigger"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    fireEvent.click(await screen.findByRole("button", { name: "confirm-delete" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: "/acme/workspace-views/all-issues" });
  });

  it("adopts the row's update into the detail", async () => {
    renderScreen(CUSTOM_A);
    await screen.findByTestId("workspace-view-header");
    const updated = makeView({
      id: CUSTOM_A,
      project: null,
      name: "Renamed",
      display_filters: { layout: "spreadsheet", order_by: "priority" },
      rich_filters: { priority: "low" },
    });
    filtersHarness.props?.onViewUpdated?.(updated as never);
    await waitFor(() => expect(screen.getByRole("button", { name: "Renamed" })).toBeInTheDocument());
    expect(listHarness.query).toMatchObject({
      orderBy: "priority",
      filters: { filters: '{"priority":"low"}' },
    });
  });

  it("switches to a default from a custom screen", async () => {
    renderScreen(CUSTOM_A);
    fireEvent.click(await screen.findByRole("button", { name: "Triage" }));
    fireEvent.click(await screen.findByRole("option", { name: "Assigned" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({ to: "/acme/workspace-views/assigned" });
  });

  it("navigates to the row's saved copy", async () => {
    renderScreen(CUSTOM_A);
    await screen.findByTestId("workspace-view-header");
    filtersHarness.props?.onViewCreated?.(makeView({ id: CUSTOM_B, project: null, name: "Later" }) as never);
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/workspace-views/${CUSTOM_B}` })
    );
  });

  it("adopts row writes without a list entry when the cache was evicted", async () => {
    const { queryClient } = renderScreen(CUSTOM_A);
    await screen.findByTestId("workspace-view-header");
    queryClient.removeQueries({ queryKey: ["ws", "acme", "views", "workspace", "list"] });
    filtersHarness.props?.onViewUpdated?.(makeView({ id: CUSTOM_A, project: null, name: "Renamed" }) as never);
    await waitFor(() => expect(screen.getByRole("button", { name: "Renamed" })).toBeInTheDocument());
    filtersHarness.props?.onViewCreated?.(makeView({ id: CUSTOM_B, project: null, name: "Later" }) as never);
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({ to: `/acme/workspace-views/${CUSTOM_B}` })
    );
  });

  it("carries local display edits into the row's snapshot", async () => {
    renderScreen(CUSTOM_A);
    await screen.findByTestId("workspace-view-header");
    const before = filtersHarness.props?.viewDisplay as { orderBy?: string } | undefined;
    expect(before).toMatchObject({ orderBy: "-created_at" });
    (displayHarness.props?.onDisplayChange as (next: unknown) => void)?.({
      ...(displayHarness.props?.display as Record<string, unknown>),
      orderBy: "priority",
    });
    await waitFor(() => expect(filtersHarness.props?.viewDisplay).toMatchObject({ orderBy: "priority" }));
  });
  it("renders a custom while the session loads, without owner standing", async () => {
    sessionHarness.mePending = true;
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("workspace-view-menu-trigger"));
    expect(await screen.findAllByRole("menuitem")).not.toHaveLength(0);
    expect(screen.queryByRole("menuitem", { name: "Edit" })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "Delete" })).toBeNull();
  });

  it("hides the owner entries from a member who does not own the custom", async () => {
    sessionHarness.meId = "623e4567-e89b-12d3-a456-426614174007";
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("workspace-view-menu-trigger"));
    expect(await screen.findAllByRole("menuitem")).not.toHaveLength(0);
    expect(screen.queryByRole("menuitem", { name: "Edit" })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "Delete" })).toBeNull();
  });

  it("adds a view from a custom as a member", async () => {
    renderScreen(CUSTOM_A);
    fireEvent.click(await screen.findByTestId("workspace-view-add"));
    expect(await screen.findByTestId("view-dialog")).toHaveAttribute("data-mode", "create");
  });

  it("opens the work item dialog from the custom's empty card", async () => {
    renderScreen(CUSTOM_A);
    fireEvent.click(await screen.findByRole("button", { name: "New work item" }));
    expect(await screen.findByTestId("create-dialog")).toHaveAttribute("data-projects", "1");
  });
});

describe("WorkspaceViewScreen layout selector", () => {
  it("persists a cloud switch through the display-filter update", async () => {
    editionHarness.edition = {
      id: "cloud",
      flags: {},
      slots: { workspaceViewLayouts: { layouts: ["kanban", "list"] } },
    };
    renderScreen(CUSTOM_A);
    const switcher = await screen.findByTestId("layout-switcher");
    expect(switcher).toHaveAttribute("data-active", "spreadsheet");
    fireEvent.click(screen.getByRole("button", { name: "switch-kanban" }));
    await waitFor(() => expect(harness.patches).toHaveLength(1));
    expect(harness.patches[0]).toEqual({
      display_filters: { layout: "kanban", order_by: "-created_at" },
    });
    await waitFor(() => expect(screen.getByTestId("layout-switcher")).toHaveAttribute("data-active", "kanban"));
  });

  it("sends one update for rapid switches and reports a failed one", async () => {
    editionHarness.edition = { id: "cloud", flags: {}, slots: { workspaceViewLayouts: { layouts: ["kanban"] } } };
    harness.patchStatus = 500;
    renderScreen(CUSTOM_A);
    await screen.findByTestId("layout-switcher");
    fireEvent.click(screen.getByRole("button", { name: "switch-kanban" }));
    fireEvent.click(screen.getByRole("button", { name: "switch-kanban" }));
    await waitFor(() => expect(harness.patches).toHaveLength(1));
    await waitFor(() => expect(shellHarness.notifyError).toHaveBeenCalled());
    expect(shellHarness.notifyError).toHaveBeenCalledWith(
      "Couldn't switch the layout",
      expect.stringContaining("unchanged")
    );
    expect(screen.getByTestId("layout-switcher")).toHaveAttribute("data-active", "spreadsheet");
  });

  it("keeps the member's default layout in the personal record", async () => {
    editionHarness.edition = { id: "cloud", flags: {}, slots: { workspaceViewLayouts: { layouts: ["list"] } } };
    renderScreen("assigned");
    await screen.findByTestId("layout-switcher");
    fireEvent.click(screen.getByRole("button", { name: "switch-list" }));
    await waitFor(() => expect(prefsHarness.updates).toEqual([{ layout: "list" }]));
  });

  it("renders nothing for a stored layout the surface cannot draw", async () => {
    harness.view = {
      ...makeView({ id: CUSTOM_A, project: null, name: "Triage" }),
      display_filters: { layout: "kanban" },
    };
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    expect(await screen.findByTestId("workspace-view-empty-body")).toBeInTheDocument();
    expect(screen.queryByTestId("global-list")).toBeNull();
  });

  it("renders an offered cloud layout through the surface body", async () => {
    editionHarness.edition = { id: "cloud", flags: {}, slots: { workspaceViewLayouts: { layouts: ["kanban"] } } };
    harness.view = {
      ...makeView({ id: CUSTOM_A, project: null, name: "Triage" }),
      display_filters: { layout: "kanban" },
    };
    renderScreen(CUSTOM_A);
    expect(await screen.findByTestId("global-list")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-view-empty-body")).toBeNull();
  });
});
