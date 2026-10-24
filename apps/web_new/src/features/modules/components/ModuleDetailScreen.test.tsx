// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail screen frames the shared host with the trail, the tab
// title, the badge, toggle and module actions header actions, and the
// collapsible sidebar beside the layouts (MOD-029, MOD-030, MOD-032).
// The module actions menu is mounted here (MOD-024/025); these cases drive
// its real trigger through Edit and Delete landing.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  projects: vi.fn(),
  detail: vi.fn(),
  prefs: vi.fn(),
  page: vi.fn(),
  peekChange: vi.fn(),
}));
const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerHarness.navigate,
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/shell/index.js")>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useSuspenseQuery: (options: { queryKey: readonly unknown[] }) => {
    const client = (globalThis as { __detailQueryClient?: QueryClient }).__detailQueryClient;
    return { data: client?.getQueryData(options.queryKey as never) };
  },
}));

vi.mock("../../projects/index.js", () => ({ projectsQueryOptions: mocks.projects }));
vi.mock("./sidebar/moduleSidebarQueries.js", () => ({ moduleDetailQueryOptions: mocks.detail }));
vi.mock("../../issues/prefs/prefsQuery.js", () => ({ prefsQueryOptions: mocks.prefs }));
vi.mock("../../issues/list/pages.js", () => ({ listPageQueryOptions: mocks.page }));

vi.mock("./ModuleAddWorkItem.js", () => ({
  ModuleAddWorkItem: (props: {
    project: { id: string; memberRole: number | null; archived: boolean };
    moduleId: string;
    controller: { canAdd: boolean };
  }) => (
    <div
      data-testid="add-work-item-stub"
      data-project={props.project.id}
      data-role={String(props.project.memberRole)}
      data-archived={String(props.project.archived)}
      data-module={props.moduleId}
      data-can-add={String(props.controller.canAdd)}
    />
  ),
}));

vi.mock("./ModuleDetailCrumbs.js", () => ({
  ModuleDetailCrumbs: (props: { projectName: string; moduleName: string }) => (
    <div data-testid="crumbs-stub" data-project={props.projectName} data-module={props.moduleName} />
  ),
}));

vi.mock("../../issues/list/EntityListScreen.js", () => ({
  EntityListScreen: (props: {
    entity: unknown;
    title: string;
    moduleId: string;
    completedModule: boolean;
    peekKey: string | null;
    compact: boolean;
    extraHeaderActions: React.ReactNode;
    emptyStateActions: { canAct: boolean; onCreate: () => void; onAddExisting: () => void };
    onPeekChange: (key: string | null) => void;
  }) => (
    <div
      data-testid="entity-stub"
      data-entity={JSON.stringify(props.entity)}
      data-title={props.title}
      data-module={props.moduleId}
      data-module-completed={String(props.completedModule)}
      data-peek={String(props.peekKey ?? "")}
      data-compact={String(props.compact)}
    >
      <button type="button" onClick={() => props.onPeekChange("WEB-1")}>
        peek
      </button>
      <div data-testid="extra-actions">{props.extraHeaderActions}</div>
      <div data-testid="empty-actions" data-can-act={String(props.emptyStateActions.canAct)} />
    </div>
  ),
}));

vi.mock("./sidebar/ModuleAnalyticsSidebar.js", () => ({
  ModuleAnalyticsSidebar: (props: { moduleId: string; onClose?: () => void }) => (
    <div data-testid="sidebar-stub" data-module={props.moduleId}>
      {props.onClose === undefined ? null : (
        <button type="button" data-testid="sidebar-close" onClick={props.onClose}>
          close
        </button>
      )}
    </div>
  ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { jsonResponse, makeModuleRow } from "../testing.js";
import { ModuleDetailScreen } from "./ModuleDetailScreen.js";
import { MODULE_DETAIL_SIDEBAR_STORAGE_KEY } from "../lib/detailSidebarStore.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const PROJECTS_KEY = ["projects"] as const;
const DETAIL_KEY = ["detail"] as const;
const PREFS_KEY = ["prefs"] as const;
const PAGE_KEY = ["page"] as const;

function project(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    member_role: 20,
    archived_at: null,
    cycle_view: true,
    module_view: true,
    ...overrides,
  };
}

function detail(overrides: Record<string, unknown> = {}) {
  return makeModuleRow({
    id: MODULE,
    project_id: PROJECT,
    name: "Delivery",
    status: "in-progress",
    ...overrides,
  });
}

const PROJECT_PATH = `/api/workspaces/acme/projects/${PROJECT}/`;
const DETAIL_PATH = `${PROJECT_PATH}modules/${MODULE}/`;

function factsProject(overrides: Record<string, unknown> = {}) {
  return {
    ...project(),
    timezone: "UTC",
    ...overrides,
  };
}

/** The transport the mounted menu reads through; the cached reads above
 * stay mocked, so this only answers the detail, facts and picker reads. */
function stubTransport(state: { deleteStatus: number }, overrides: Record<string, unknown> = {}) {
  return (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === `https://api.test${PROJECT_PATH}`) return jsonResponse(url, 200, factsProject(overrides));
    if (url === `https://api.test${DETAIL_PATH}` && method === "GET") return jsonResponse(url, 200, detail());
    if (url === `https://api.test${DETAIL_PATH}` && method === "DELETE") {
      if (state.deleteStatus === 204) return jsonResponse(url, 204, null);
      return jsonResponse(url, state.deleteStatus, { detail: "Refused." });
    }
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, []);
    if (url.includes("/members/") && url.includes("/projects/")) return jsonResponse(url, 200, []);
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 500, {});
    return jsonResponse(url, 500, {});
  }) as never;
}

function setup(
  options: {
    projects?: Array<Record<string, unknown>>;
    total?: number;
    status?: string;
    deleteStatus?: number;
  } = {}
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  (globalThis as { __detailQueryClient?: QueryClient }).__detailQueryClient = queryClient;
  queryClient.setQueryData(PROJECTS_KEY, options.projects ?? [project()]);
  queryClient.setQueryData(DETAIL_KEY, detail(options.status === undefined ? {} : { status: options.status }));
  queryClient.setQueryData(PREFS_KEY, {
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
  });
  queryClient.setQueryData(PAGE_KEY, { total_count: options.total ?? 7, results: [], count: 0 });
  mocks.peekChange.mockClear();
  deleteState.deleteStatus = options.deleteStatus ?? 204;
  createAppClient("https://api.test", stubTransport(deleteState));
  return render(
    <QueryClientProvider client={queryClient}>
      <ModuleDetailScreen
        workspaceSlug="acme"
        projectId={PROJECT}
        moduleId={MODULE}
        peekKey={null}
        onPeekChange={mocks.peekChange}
        compact={false}
      />
    </QueryClientProvider>
  );
}

let deleteState: { deleteStatus: number };

beforeEach(() => {
  vi.clearAllMocks();
  deleteState = { deleteStatus: 204 };
  routerHarness.navigate.mockReset();
  notify.notifyError.mockReset();
  notify.notifySuccess.mockReset();
  globalThis.localStorage.clear();
  document.title = "";
  mocks.projects.mockReturnValue({ queryKey: PROJECTS_KEY, queryFn: vi.fn() });
  mocks.detail.mockReturnValue({ queryKey: DETAIL_KEY, queryFn: async () => detail() });
  mocks.prefs.mockReturnValue({ queryKey: PREFS_KEY, queryFn: vi.fn() });
  mocks.page.mockReturnValue({ queryKey: PAGE_KEY, queryFn: vi.fn() });
});

afterEach(() => {
  resetAppClient();
});

describe("ModuleDetailScreen", () => {
  it("frames the host with the trail, title, badge and sidebar", () => {
    setup();
    expect(screen.getByTestId("module-detail-screen")).toBeInTheDocument();
    const crumbs = screen.getByTestId("crumbs-stub");
    expect(crumbs).toHaveAttribute("data-project", "Website");
    expect(crumbs).toHaveAttribute("data-module", "Delivery");
    expect(document.title).toBe("Website - Delivery");
    const host = screen.getByTestId("entity-stub");
    expect(host).toHaveAttribute(
      "data-entity",
      JSON.stringify({ scope: "module", projectId: PROJECT, moduleId: MODULE })
    );
    expect(host).toHaveAttribute("data-title", "Delivery");
    expect(host).toHaveAttribute("data-module", MODULE);
    expect(host).toHaveAttribute("data-module-completed", "false");
    const actions = screen.getByTestId("extra-actions");
    expect(within(actions).getByTestId("module-detail-badge")).toHaveTextContent("7");
    expect(within(actions).getByTestId("module-detail-sidebar-toggle")).toBeInTheDocument();
    expect(screen.getByTestId("sidebar-stub")).toHaveAttribute("data-module", MODULE);
  });

  it("reads the badge from the unfiltered first page", () => {
    setup();
    expect(mocks.page).toHaveBeenCalledWith(expect.anything(), "acme", {
      entity: { scope: "module", projectId: PROJECT, moduleId: MODULE },
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues: true,
    });
  });

  it("hides the badge on an empty module", () => {
    setup({ total: 0 });
    expect(screen.queryByTestId("module-detail-badge")).not.toBeInTheDocument();
    expect(screen.getByTestId("module-detail-sidebar-toggle")).toBeInTheDocument();
  });

  it("mounts the add entry for an editing standing and hands the empty card the same rule", () => {
    setup();
    const add = screen.getByTestId("add-work-item-stub");
    expect(add).toHaveAttribute("data-project", PROJECT);
    expect(add).toHaveAttribute("data-module", MODULE);
    expect(add).toHaveAttribute("data-can-add", "true");
    // The API row's member_role/archived_at reach the dialog as the option's
    // memberRole/archived, so its project filter keeps the lone project.
    expect(add).toHaveAttribute("data-role", "20");
    expect(add).toHaveAttribute("data-archived", "false");
    expect(screen.getByTestId("empty-actions")).toHaveAttribute("data-can-act", "true");
  });

  it("maps an archived project row to the dialog's archived flag", () => {
    setup({ projects: [project({ archived_at: "2026-01-02T03:04:05Z" })] });
    expect(screen.queryByTestId("add-work-item-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("empty-actions")).toHaveAttribute("data-can-act", "false");
  });

  it("mounts no add entry for a guest and disables the empty card's actions", () => {
    setup({ projects: [project({ member_role: 5 })] });
    expect(screen.queryByTestId("add-work-item-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("empty-actions")).toHaveAttribute("data-can-act", "false");
  });

  it("keeps the add entries on a completed module, as the old header did", () => {
    setup({ status: "completed" });
    expect(screen.getByTestId("add-work-item-stub")).toHaveAttribute("data-can-add", "true");
    expect(screen.getByTestId("empty-actions")).toHaveAttribute("data-can-act", "true");
  });

  it("marks a completed module read-only for the host", () => {
    setup({ status: "completed" });
    expect(screen.getByTestId("entity-stub")).toHaveAttribute("data-module-completed", "true");
  });

  it("forwards peek changes to the route", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "peek" }));
    expect(mocks.peekChange).toHaveBeenCalledWith("WEB-1");
  });

  it("collapses and expands the sidebar through the header toggle, persisting the choice", () => {
    setup();
    const toggle = screen.getByTestId("module-detail-sidebar-toggle");
    expect(toggle).toHaveAccessibleName("Collapse sidebar");
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe(null);
    fireEvent.click(toggle);
    expect(screen.queryByTestId("sidebar-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("module-detail-sidebar-toggle")).toHaveAccessibleName("Expand sidebar");
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("true");
    fireEvent.click(screen.getByTestId("module-detail-sidebar-toggle"));
    expect(screen.getByTestId("sidebar-stub")).toBeInTheDocument();
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("false");
  });

  it("restores a collapsed sidebar as hidden", () => {
    globalThis.localStorage.setItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY, "true");
    setup();
    expect(screen.queryByTestId("sidebar-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("module-detail-sidebar-toggle")).toHaveAccessibleName("Expand sidebar");
  });

  it("collapses through the sidebar's own close control", () => {
    setup();
    fireEvent.click(screen.getByTestId("sidebar-close"));
    expect(screen.queryByTestId("sidebar-stub")).not.toBeInTheDocument();
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("true");
  });

  it("renders the project-not-found state when the project is missing", () => {
    setup({ projects: [] });
    expect(screen.getByRole("heading", { name: "Project not found" })).toBeInTheDocument();
    expect(screen.queryByTestId("entity-stub")).not.toBeInTheDocument();
    expect(document.title).toBe("Modules");
  });

  it("mounts the actions menu inside the header actions", () => {
    setup();
    const actions = screen.getByTestId("extra-actions");
    expect(within(actions).getByRole("button", { name: "Module actions" })).toBeInTheDocument();
  });

  it("opens Update module from the real detail trigger, prefilled and with no project picker", async () => {
    setup();
    const actions = screen.getByTestId("extra-actions");
    const trigger = within(actions).getByRole("button", { name: "Module actions" });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Edit" }));
    expect(await screen.findByRole("heading", { name: "Update module" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Delivery");
    expect(screen.queryByTestId("module-dialog-project")).not.toBeInTheDocument();
  });

  it("deletes from the real detail trigger and lands on the plain modules list", async () => {
    setup();
    const actions = screen.getByTestId("extra-actions");
    const trigger = within(actions).getByRole("button", { name: "Module actions" });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "Delete module" })).toBeInTheDocument();
    expect(screen.getByTestId("module-delete-body")).toHaveTextContent("Delivery");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({
        to: `/acme/projects/${PROJECT}/modules/`,
        search: {},
        replace: true,
      })
    );
    expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", expect.stringContaining("Delivery"));
  });

  it("keeps the module in place when the delete is refused", async () => {
    setup({ deleteStatus: 403 });
    const actions = screen.getByTestId("extra-actions");
    const trigger = within(actions).getByRole("button", { name: "Module actions" });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("menuitem", { name: "Delete" }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith("You don't have permission to perform this action.", "Refused.")
    );
    expect(routerHarness.navigate).not.toHaveBeenCalled();
  });

  it("shows no actions menu to a guest", () => {
    setup({ projects: [project({ member_role: 5 })] });
    const actions = screen.getByTestId("extra-actions");
    expect(within(actions).queryByRole("button", { name: "Module actions" })).not.toBeInTheDocument();
  });

  it("shows no actions menu for a visitor outside the project", () => {
    setup({ projects: [project({ member_role: null })] });
    const actions = screen.getByTestId("extra-actions");
    expect(within(actions).queryByRole("button", { name: "Module actions" })).not.toBeInTheDocument();
  });

  it("hides the actions menu on an archived project", () => {
    setup({ projects: [project({ archived_at: "2026-01-02T03:04:05Z" })] });
    const actions = screen.getByTestId("extra-actions");
    expect(within(actions).queryByRole("button", { name: "Module actions" })).not.toBeInTheDocument();
  });
});
