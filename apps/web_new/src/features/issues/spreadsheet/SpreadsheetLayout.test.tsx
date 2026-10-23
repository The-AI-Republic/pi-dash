// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import type { IssueListItem, IssueListResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { IssuesLoaderProvider } from "../loaders/loaderContext.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { edition } from "../../../core/edition/index.js";
import { BULK_OPERATIONS_FLAG } from "../bulk/edition.js";
import { buildDisplayRows, focusSheetCell, SpreadsheetLayout, useViewportWindow } from "./SpreadsheetLayout.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { SHEET_COLUMNS } from "./model.js";

const bulkMocks = vi.hoisted(() => ({
  update: vi.fn(),
  remove: vi.fn(),
  archive: vi.fn(),
  subscribe: vi.fn(),
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
    onUpdate: (properties: unknown) => void;
    onDelete: () => void;
    onArchive: () => void;
    onSubscribe: () => void;
    onClear: () => void;
  }) => (
    <div data-testid="bulk-toolbar">
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
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ID1 = "123e4567-e89b-12d3-a456-426614174000";
const ID2 = "323e4567-e89b-12d3-a456-426614174002";
const ID3 = "423e4567-e89b-12d3-a456-426614174003";
const ID4 = "523e4567-e89b-12d3-a456-426614174004";
const ENTITY = { scope: "project", projectId: PROJECT } as const;

function prefs(overrides: Partial<DisplayPrefs> = {}): DisplayPrefs {
  return {
    layout: "spreadsheet",
    groupBy: null,
    subGroupBy: null,
    orderBy: "-created_at",
    showEmptyGroups: false,
    showSubIssues: false,
    calendar: { mode: "month", showWeekends: true },
    properties: {},
    ...overrides,
  };
}

function issue(id: string, name: string, sequence: number, overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id,
    name,
    state_id: null,
    sort_order: sequence,
    completed_at: null,
    priority: "none",
    sequence_id: sequence,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function page(results: IssueListItem[], overrides: Partial<IssueListResponse> = {}): IssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
    ...overrides,
  };
}

interface Stub {
  calls: Array<{ url: string; method: string; body: string }>;
  pages: IssueListResponse[];
  children: Record<string, IssueListItem[]>;
  hold?: boolean;
  release?: (() => void) | undefined;
  cycles?: "ok" | "completed" | "pending";
  failCreate?: boolean;
}

function stub(options: Partial<Stub> = {}): Stub {
  const state: Stub = {
    calls: [],
    pages: [page([issue(ID1, "Fix login", 1), issue(ID2, "Second", 2)])],
    children: {},
    ...options,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    state.calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (state.hold === true && (url.includes("cursor=next") || (init.method === "POST" && url.endsWith("/issues/")))) {
      await new Promise<void>((resolve) => (state.release = resolve));
    }
    let status = 200;
    let body: unknown = {};
    if (state.cycles === "pending" && url.includes("/cycles/")) {
      await new Promise<void>(() => undefined);
    }
    if (state.failCreate === true && init.method === "POST" && url.includes(`/projects/${PROJECT}/issues/`)) {
      status = 400;
      body = { error: "refused" };
    } else if (url.endsWith("/auth/get-csrf-token/")) body = { csrf_token: "csrf" };
    else if (url.includes("/sub-issues/")) {
      const parent = url.split("/issues/")[1]?.split("/")[0] ?? "";
      body = { sub_issues: state.children[parent] ?? [], state_distribution: {} };
    } else if (url.includes(`/projects/${PROJECT}/issues/`) && !url.includes("/sub-issues/")) {
      const parsed = new URL(url, "https://api.test");
      body =
        parsed.searchParams.get("cursor") === null
          ? state.pages[0]
          : page(state.pages.slice(1).flatMap((entry) => entry.results));
      status = 200;
    } else if (url.includes("/user-properties/")) {
      body = ProjectPrefsPayload;
    } else if (url.includes("/cycles/")) {
      body = [
        {
          id: ID2,
          project_id: PROJECT,
          name: "Cycle 1",
          status: state.cycles === "completed" ? "COMPLETED" : "ACTIVE",
          is_favorite: false,
        },
      ];
    } else if (url.includes("/modules/")) {
      body = [{ id: ID3, project_id: PROJECT, name: "Module 1", status: "active", is_favorite: false }];
    } else if (init.method === "PATCH") {
      status = 201;
      body = ProjectPrefsPayload;
    }
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return state;
}

const ProjectPrefsPayload = {
  id: ID3,
  workspace: ID3,
  project: PROJECT,
  user: ID3,
  filters: {},
  display_filters: { layout: "spreadsheet", group_by: null, order_by: "-created_at" },
  display_properties: {},
  rich_filters: {},
  preferences: {},
  sort_order: 1,
  created_by: null,
  updated_by: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function renderSheet(
  props: {
    prefs?: DisplayPrefs;
    canEdit?: boolean;
    hideCycle?: boolean;
    hideModule?: boolean;
    hideEstimate?: boolean;
    highlightedId?: string | null;
    weekStart?: number;
    enableSelection?: boolean;
  } = {}
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rootRoute = createRootRoute({
    component: () => (
      <IssuesLoaderProvider>
        <Outlet />
      </IssuesLoaderProvider>
    ),
  });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const browseRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "browse/$issueKey",
    component: () => <p>Browse</p>,
  });
  const sheetRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/issues",
    component: () => (
      <SpreadsheetLayout
        workspaceSlug={SLUG}
        projectId={PROJECT}
        projectIdentifier="WEB"
        entity={ENTITY}
        prefs={props.prefs ?? prefs()}
        canEdit={props.canEdit ?? true}
        hideCycle={props.hideCycle ?? false}
        hideModule={props.hideModule ?? false}
        hideEstimate={props.hideEstimate ?? false}
        highlightedId={props.highlightedId ?? null}
        weekStart={props.weekStart ?? 0}
        enableSelection={props.enableSelection ?? true}
      />
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([browseRoute, sheetRoute])]),
    history: createMemoryHistory({ initialEntries: [`/${SLUG}/projects/${PROJECT}/issues`] }),
    defaultPreload: false,
  });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { ...result, queryClient, router };
}

/**
 * An in-memory Storage for the sort persistence cases. Runners where the
 * host has no localStorage (Node without a file backend) still get a
 * real store to assert against; jsdom's own store is untouched.
 */
function sortStore(): Storage {
  const entries = new Map<string, string>();
  return {
    length: entries.size,
    clear: () => entries.clear(),
    getItem: (key: string) => entries.get(key) ?? null,
    key: (index: number) => [...entries.keys()][index] ?? null,
    removeItem: (key: string) => {
      entries.delete(key);
    },
    setItem: (key: string, value: string) => {
      entries.set(key, value);
    },
  };
}

function setViewport(width: number, height: number): void {
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("data-testid") === "spreadsheet-scroller") return height;
      if (this.tagName === "DIV" && this.parentElement?.getAttribute("data-testid") === "spreadsheet-scroller") {
        return height;
      }
      return 0;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => width });
}

beforeEach(() => {
  for (const action of Object.values(bulkMocks)) action.mockResolvedValue(undefined);
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div><div id="menu-portal"></div>`;
  // Runners without a storage backend (Node's bare localStorage global)
  // still mount the sheet; the sort persistence cases opt into jsdom's.
  globalThis.localStorage?.clear();
  setViewport(800, 400);
});

afterEach(() => {
  delete edition.flags[BULK_OPERATIONS_FLAG];
  resetAppClient();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("SpreadsheetLayout", () => {
  it("renders the header order and the enabled display columns", async () => {
    stub();
    renderSheet({ prefs: prefs({ properties: { labels: false, cycle: false, modules: false } }) });
    await screen.findByRole("table");
    const headers = screen.getAllByRole("columnheader").map((header) => header.textContent);
    expect(headers[0]).toBe("Work items");
    expect(headers).not.toContain("Labels");
    expect(headers).not.toContain("Cycle");
    expect(headers).not.toContain("Modules");
    expect(headers).toContain("State");
    expect(headers).toContain("Due date");
  });

  it("hides cycle and modules when the project features are off", async () => {
    stub();
    renderSheet({ hideCycle: true, hideModule: true });
    await screen.findByRole("table");
    const headers = screen.getAllByRole("columnheader").map((header) => header.textContent);
    expect(headers).not.toContain("Cycle");
    expect(headers).not.toContain("Modules");
  });

  it("hides the estimate column when the project has no estimate system", async () => {
    stub();
    renderSheet({ hideEstimate: true });
    await screen.findByRole("table");
    expect(screen.getAllByRole("columnheader").map((header) => header.textContent)).not.toContain("Estimate");
  });

  it("keeps the first column and header sticky and shadows the column on scroll", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    const table = screen.getByRole("table");
    expect(within(table).getAllByRole("columnheader")[0]).toHaveClass("sticky");
    expect(table.querySelector("thead")).toHaveClass("sticky");
    await waitFor(() => expect(table.querySelector(`td#issue-${ID1}`)).not.toBeNull());
    const first = table.querySelector(`td#issue-${ID1}`) as HTMLElement;
    expect(first).toHaveClass("sticky");
    expect(first.style.boxShadow).toBe("none");
    fireEvent.scroll(screen.getByTestId("spreadsheet-scroller"), { target: { scrollLeft: 100 } });
    await waitFor(() => expect(first.style.boxShadow).not.toBe("none"));
  });

  it("renders each row's identifier and title and highlights requested rows", async () => {
    stub();
    renderSheet({ highlightedId: ID2 });
    await screen.findByRole("table");
    await waitFor(() => expect(screen.getByText("Fix login")).toBeInTheDocument());
    expect(screen.getByText("Second")).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Second/ })).toHaveAttribute("data-highlight", "true");
  });

  it("reserves space for rows below the viewport with a bottom spacer", async () => {
    const rows = Array.from({ length: 20 }, (_, index) =>
      issue(`123e4567-e89b-12d3-a456-${String(426614174000 + index)}`, `Fix login ${index}`, index + 1)
    );
    stub({ pages: [page(rows)] });
    renderSheet();
    await screen.findByRole("table");
    await waitFor(() => expect(screen.getByText("Fix login 0")).toBeInTheDocument());
    const spacer = await waitFor(() => {
      const node = screen.getByRole("table").querySelector("tbody tr[aria-hidden='true']") as HTMLElement | null;
      expect(node).not.toBeNull();
      return node;
    });
    expect(spacer).not.toBeNull();
    expect(spacer?.querySelector("td")?.style.height).not.toBe("");
  });

  it("offers ascending, descending and clear on a sorted header, then writes order_by", async () => {
    const store = sortStore();
    vi.stubGlobal("localStorage", store);
    const state = stub();
    renderSheet();
    await screen.findByRole("table");
    const stateHeader = screen.getByRole("columnheader", { name: /State/ });
    fireEvent.click(within(stateHeader).getByRole("button"));
    const items = screen.getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Ascending", "Descending"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    await waitFor(() => expect(state.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(store.getItem("web_new.spreadsheet.sort")).toContain("state__name");
  });

  it("writes a descending order from the header menu", async () => {
    const state = stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(within(screen.getByRole("columnheader", { name: /State/ })).getByRole("button"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Descending" }));
    await waitFor(() =>
      expect(state.calls.some((call) => call.method === "PATCH" && call.body.includes("-state__name"))).toBe(true)
    );
  });

  it("adds Clear sorting once sorted and clears to creation order", async () => {
    const store = sortStore();
    vi.stubGlobal("localStorage", store);
    const state = stub();
    renderSheet({ prefs: prefs({ orderBy: "state__name" }) });
    await screen.findByRole("table");
    const stateHeader = screen.getByRole("columnheader", { name: /State/ });
    fireEvent.click(within(stateHeader).getByRole("button"));
    expect(screen.getByRole("menuitem", { name: /Clear sorting/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: /Clear sorting/ }));
    await waitFor(() =>
      expect(state.calls.some((call) => call.method === "PATCH" && call.body.includes("-created_at"))).toBe(true)
    );
    expect(store.getItem("web_new.spreadsheet.sort")).toBe("");
  });

  it("marks a server sort that has no stored local pair and re-picking it refreshes only the pair", async () => {
    const store = sortStore();
    vi.stubGlobal("localStorage", store);
    const state = stub();
    renderSheet({ prefs: prefs({ orderBy: "state__name" }) });
    await screen.findByRole("table");
    const stateHeader = screen.getByRole("columnheader", { name: /State/ });
    expect(within(stateHeader).getByRole("button").querySelector("svg")).not.toBeNull();
    fireEvent.click(within(stateHeader).getByRole("button"));
    expect(screen.getByRole("menuitem", { name: /Clear sorting/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    expect(store.getItem("web_new.spreadsheet.sort")).toContain("state__name");
    expect(state.calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("marks a descending sort and writes both cycle and module bridges", async () => {
    const state = stub();
    renderSheet({ prefs: prefs({ orderBy: "-state__name" }) });
    await screen.findByRole("table");
    const stateHeader = screen.getByRole("columnheader", { name: /State/ });
    expect(within(stateHeader).getByRole("button").querySelector("svg")).not.toBeNull();
    fireEvent.click(within(stateHeader).getByRole("button"));
    expect(screen.getByRole("menuitem", { name: /Clear sorting/ })).toBeInTheDocument();

    fireEvent.click(document.querySelector(`#sheet-cell-${ID1}-cycle button`) as HTMLElement);
    await screen.findByRole("option", { name: /Cycle 1/ });
    fireEvent.click(screen.getByRole("option", { name: /Cycle 1/ }));
    await waitFor(() => expect(state.calls.some((call) => call.url.includes("/cycles/"))).toBe(true));

    fireEvent.click(
      within(document.getElementById(`sheet-cell-${ID1}-modules`) as HTMLElement).getByRole("button", {
        name: /modules/i,
      })
    );
    await screen.findByRole("option", { name: /Module 1/ });
    fireEvent.click(screen.getByRole("option", { name: /Module 1/ }));
    await waitFor(() => expect(state.calls.some((call) => call.url.includes("/modules/"))).toBe(true));
  });

  it("restores a remembered created-on ascending sort after reload", async () => {
    const store = sortStore();
    store.setItem(
      "web_new.spreadsheet.sort",
      JSON.stringify({ column: "created_on", direction: "ascending", orderBy: "-created_at" })
    );
    vi.stubGlobal("localStorage", store);
    stub();
    renderSheet();
    await screen.findByRole("table");
    const createdHeader = screen.getByRole("columnheader", { name: /Created on/ });
    fireEvent.click(within(createdHeader).getByRole("button"));
    expect(screen.getByRole("menuitem", { name: /Clear sorting/ })).toBeInTheDocument();
  });

  it("opens quick-add, submits on Enter and keeps the form open", async () => {
    const state = stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    const input = screen.getByPlaceholderText("Work item title");
    fireEvent.change(input, { target: { value: "New sheet item" } });
    fireEvent.submit(input.closest("form")!);
    await waitFor(() => expect(state.calls.some((call) => call.method === "POST")).toBe(true));
    expect(screen.getByPlaceholderText("Work item title")).toBeInTheDocument();
  });

  it("renders the pending quick-add row and ignores an empty submission", async () => {
    const state = stub();
    state.hold = true;
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    const input = screen.getByPlaceholderText("Work item title");
    fireEvent.submit(input.closest("form")!);
    expect(state.calls.some((call) => call.method === "POST")).toBe(false);
    fireEvent.change(input, { target: { value: "Pending row" } });
    fireEvent.submit(input.closest("form")!);
    const pending = await screen.findByText("Pending row");
    expect(pending.closest("tr")).toHaveAttribute("data-temprow", "true");
    state.release?.();
    await waitFor(() => expect(screen.queryByText("Pending row")).not.toBeInTheDocument());
  });

  it("keeps quick-add open and ready after a refused create", async () => {
    stub({ failCreate: true });
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    const input = screen.getByPlaceholderText("Work item title");
    fireEvent.change(input, { target: { value: "Refused item" } });
    fireEvent.submit(input.closest("form")!);
    await waitFor(() => expect(document.activeElement).toBe(screen.getByPlaceholderText("Work item title")));
    expect(screen.getByPlaceholderText("Work item title")).toBeInTheDocument();
  });

  it("closes quick-add on Escape", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    fireEvent.keyDown(screen.getByPlaceholderText("Work item title"), { key: "Escape" });
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();
  });

  it("closes quick-add on an outside pointer press", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("button", { name: "Add work item" }));
    expect(screen.getByPlaceholderText("Work item title")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByPlaceholderText("Work item title")).not.toBeInTheDocument();
  });

  it("disables quick-add without edit permission", async () => {
    stub();
    renderSheet({ canEdit: false });
    await screen.findByRole("table");
    expect(screen.getByRole("button", { name: "Add work item" })).toBeDisabled();
  });

  it("loads the next page when the viewport reaches the bottom", async () => {
    const firstRows = Array.from({ length: 20 }, (_, index) => issue(ID1, `Fix login ${index}`, index + 1));
    const first = page(firstRows, { next_page_results: true, next_cursor: "next", total_count: 21 });
    const second = page([issue(ID2, "Second", 21)], { total_count: 21 });
    const state = stub({ pages: [first, second] });
    renderSheet();
    await screen.findByRole("table");
    const scroller = screen.getByTestId("spreadsheet-scroller");
    Object.defineProperty(scroller, "scrollHeight", { value: 880, configurable: true });
    Object.defineProperty(scroller, "scrollTop", { value: 472, configurable: true });
    fireEvent.scroll(scroller);
    await waitFor(() => expect(state.calls.some((call) => call.url.includes("cursor=next"))).toBe(true));
  });

  it("renders skeleton rows while the next page loads", async () => {
    const firstRows = Array.from({ length: 20 }, (_, index) => issue(ID1, `Fix login ${index}`, index + 1));
    const first = page(firstRows, { next_page_results: true, next_cursor: "next", total_count: 21 });
    const second = page([issue(ID2, "Second", 21)], { total_count: 21 });
    const state = stub({ pages: [first, second] });
    state.hold = true;
    renderSheet();
    await screen.findByRole("table");
    const scroller = screen.getByTestId("spreadsheet-scroller");
    Object.defineProperty(scroller, "scrollHeight", { value: 880, configurable: true });
    Object.defineProperty(scroller, "scrollTop", { value: 472, configurable: true });
    fireEvent.scroll(scroller);
    await waitFor(() => expect(document.querySelectorAll("tfoot [data-skeleton-row]")).toHaveLength(3));
    state.release?.();
  });

  it("does not request another page while one is already loading", async () => {
    const firstRows = Array.from({ length: 20 }, (_, index) => issue(ID1, `Fix login ${index}`, index + 1));
    const first = page(firstRows, { next_page_results: true, next_cursor: "next", total_count: 21 });
    const second = page([issue(ID2, "Second", 21)], { total_count: 21 });
    const state = stub({ pages: [first, second], hold: true });
    renderSheet();
    await screen.findByRole("table");
    const scroller = screen.getByTestId("spreadsheet-scroller");
    Object.defineProperty(scroller, "scrollHeight", { value: 880, configurable: true });
    Object.defineProperty(scroller, "scrollTop", { value: 472, configurable: true });
    fireEvent.scroll(scroller);
    await waitFor(() => expect(document.querySelectorAll("tfoot [data-skeleton-row]")).toHaveLength(3));
    fireEvent.scroll(scroller);
    expect(state.calls.filter((call) => call.url.includes("cursor=next"))).toHaveLength(1);
    state.release?.();
  });

  it("navigates from the sub-issue count and from the row action to the work item", async () => {
    stub({ pages: [page([issue(ID1, "Fix login", 1, { sub_issues_count: 2 }), issue(ID2, "Second", 2)])] });
    const { router } = renderSheet();
    await screen.findByRole("table");
    await waitFor(() => expect(screen.getByRole("button", { name: "2 sub-work items" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "2 sub-work items" }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`/${SLUG}/browse/WEB-1`));
    expect(router.state.location.hash.replace(/^#/, "")).toBe("sub-issues");
    await router.navigate({ to: "/$ws/projects/$projectId/issues", params: { ws: SLUG, projectId: PROJECT } });
    await screen.findByRole("table");
    fireEvent.click(await screen.findByRole("button", { name: "Open Fix login" }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`/${SLUG}/browse/WEB-1`));
    expect(router.state.location.hash).toBe("");
  });

  it("expands a sub-issue inline and indents the child", async () => {
    const parent = issue(ID1, "Parent", 1, { sub_issues_count: 1 });
    const child = { ...issue(ID2, "Child", 2), state_group: null };
    const state = stub({ pages: [page([parent])], children: { [ID1]: [child] } });
    renderSheet();
    await screen.findByRole("table");
    await waitFor(() => expect(screen.getByRole("button", { name: "Expand sub-work items" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Expand sub-work items" }));
    await waitFor(() => expect(state.calls.some((call) => call.url.includes("/sub-issues/"))).toBe(true));
    await screen.findByText("Child");
    const childCell = document.getElementById(`issue-${ID2}`) as HTMLElement;
    const spacer = childCell.querySelector('div[style*="width"]') as HTMLElement;
    expect(spacer.style.width).toBe("12px");
  });

  it("opens peek instead of expanding past three levels", async () => {
    const parent = issue(ID1, "Parent", 1, { sub_issues_count: 1 });
    const child = { ...issue(ID2, "Child", 2, { parent_id: ID1, sub_issues_count: 1 }), state_group: null };
    const grandchild = { ...issue(ID3, "Grandchild", 3, { parent_id: ID2, sub_issues_count: 1 }), state_group: null };
    const greatGrandchild = {
      ...issue(ID4, "Great-grandchild", 4, { parent_id: ID3, sub_issues_count: 1 }),
      state_group: null,
    };
    stub({
      pages: [page([parent])],
      children: { [ID1]: [child], [ID2]: [grandchild], [ID3]: [greatGrandchild] },
    });
    renderSheet();
    await screen.findByRole("table");
    for (const id of [ID1, ID2, ID3]) {
      await waitFor(() =>
        expect(
          document.getElementById(`issue-${id}`)?.querySelector('button[aria-label="Expand sub-work items"]')
        ).not.toBeNull()
      );
      fireEvent.click(
        document
          .getElementById(`issue-${id}`)
          ?.querySelector('button[aria-label="Expand sub-work items"]') as HTMLElement
      );
    }
    await screen.findByText("Great-grandchild");
    const deeper = document.getElementById(`issue-${ID4}`)?.querySelector('button[aria-label="Expand sub-work items"]');
    expect(deeper).not.toBeNull();
    fireEvent.click(deeper as HTMLElement);
    expect(await screen.findByTestId("issue-preview")).toBeInTheDocument();
  });

  it("moves focus between cells with arrow keys and clamps at the edges", async () => {
    stub();
    renderSheet({
      prefs: prefs({
        properties: {
          labels: false,
          cycle: false,
          modules: false,
          estimate: false,
          created_on: false,
          updated_on: false,
          link: false,
          attachment_count: false,
          sub_issue_count: false,
          assignee: false,
          start_date: false,
          due_date: false,
        },
      }),
    });
    await screen.findByRole("table");
    await waitFor(() => expect(document.getElementById(`issue-${ID1}`)).not.toBeNull());
    const first = document.getElementById(`issue-${ID1}`) as HTMLElement;
    const stateCell = document.getElementById(`sheet-cell-${ID1}-state`)!;
    stateCell.focus();
    fireEvent.keyDown(stateCell, { key: "ArrowRight" });
    expect(document.activeElement).toBe(document.getElementById(`sheet-cell-${ID1}-priority`));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    expect(document.activeElement).toBe(document.getElementById(`sheet-cell-${ID2}-priority`));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(document.getElementById(`sheet-cell-${ID2}-state`));
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(document.activeElement).toBe(stateCell);
    fireEvent.keyDown(stateCell, { key: "ArrowUp" });
    expect(document.activeElement).toBe(stateCell);
    fireEvent.keyDown(stateCell, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(first, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(first, { key: "Enter" });
    expect(document.activeElement).toBe(first);
    first.focus();
  });

  it("moves focus across the windowed slice, not only between rendered rows", async () => {
    const rows = Array.from({ length: 30 }, (_, index) =>
      issue(`123e4567-e89b-12d3-a456-${String(426614174000 + index)}`, `Row ${index}`, index + 1)
    );
    stub({ pages: [page(rows)] });
    setViewport(800, 44);
    renderSheet({
      prefs: prefs({
        properties: {
          labels: false,
          cycle: false,
          modules: false,
          estimate: false,
          created_on: false,
          updated_on: false,
          link: false,
          attachment_count: false,
          sub_issue_count: false,
          assignee: false,
          start_date: false,
          due_date: false,
        },
      }),
    });
    await screen.findByRole("table");
    const firstId = `issue-${rows[0]!.id}`;
    await waitFor(() => expect(document.getElementById(firstId)).not.toBeNull());
    document.getElementById(firstId)!.focus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
    await waitFor(() => expect(document.activeElement?.id).toBe(`sheet-cell-${rows[0]!.id}-state`));
    for (let row = 1; row <= 12; row += 1) {
      fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
      await waitFor(() => expect(document.activeElement?.id).toBe(`sheet-cell-${rows[row]!.id}-state`));
    }
    for (let row = 11; row >= 0; row -= 1) {
      fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
      await waitFor(() => expect(document.activeElement?.id).toBe(`sheet-cell-${rows[row]!.id}-state`));
    }
    const scroller = screen.getByTestId("spreadsheet-scroller");
    scroller.scrollTop = 20 * 44;
    fireEvent.scroll(scroller);
    const lastRenderedId = `issue-${rows[24]!.id}`;
    await waitFor(() => expect(document.getElementById(lastRenderedId)).not.toBeNull());
    document.getElementById(lastRenderedId)!.focus();
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    await waitFor(() => expect(document.activeElement?.id).toBe(`issue-${rows[25]!.id}`));
  });

  it("keeps cells editable until the cycle list loads", async () => {
    stub({ cycles: "pending" });
    renderSheet();
    await screen.findByRole("table");
    expect(document.getElementById(`sheet-cell-${ID1}-priority`)).toHaveAttribute("tabindex", "0");
  });

  it("treats completed-cycle rows as read-only", async () => {
    stub({ cycles: "completed", pages: [page([issue(ID1, "Locked", 1, { cycle_id: ID2 })])] });
    renderSheet();
    await screen.findByRole("table");
    await waitFor(() => expect(screen.queryByRole("button", { name: /no cycle/i })).not.toBeInTheDocument());
  });

  it("ignores arrow keys that bubble from a cell control", async () => {
    stub();
    renderSheet({
      prefs: prefs({
        properties: {
          labels: false,
          cycle: false,
          modules: false,
          estimate: false,
          created_on: false,
          updated_on: false,
          link: false,
          attachment_count: false,
          sub_issue_count: false,
          assignee: false,
          start_date: false,
          due_date: false,
        },
      }),
    });
    await screen.findByRole("table");
    const stateCell = document.getElementById(`sheet-cell-${ID1}-state`) as HTMLElement;
    stateCell.focus();
    const control = within(stateCell).getByRole("button");
    fireEvent.keyDown(control, { key: "ArrowRight" });
    expect(document.activeElement).toBe(stateCell);
  });

  it("collapses an expanded sub-issue", async () => {
    const parent = issue(ID1, "Parent", 1, { sub_issues_count: 1 });
    const child = { ...issue(ID2, "Child", 2, { parent_id: ID1 }), state_group: null };
    stub({ pages: [page([parent])], children: { [ID1]: [child] } });
    renderSheet();
    await screen.findByRole("table");
    const toggle = async (label: "Expand" | "Collapse"): Promise<HTMLElement> =>
      waitFor(() => {
        const node = document
          .getElementById(`issue-${ID1}`)
          ?.querySelector<HTMLElement>(`button[aria-label="${label} sub-work items"]`);
        expect(node).not.toBeNull();
        return node as HTMLElement;
      });
    fireEvent.click(await toggle("Expand"));
    expect(await screen.findByText("Child")).toBeInTheDocument();
    fireEvent.click(await toggle("Collapse"));
    await waitFor(() => expect(screen.queryByText("Child")).not.toBeInTheDocument());
  });

  it("returns focus to the cell after a picker write", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    const priority = document.getElementById(`sheet-cell-${ID1}-priority`) as HTMLElement;
    fireEvent.click(within(priority).getByRole("button", { name: /none/i }));
    fireEvent.click(screen.getByRole("option", { name: "Urgent" }));
    await waitFor(() => expect(document.activeElement?.id).toBe(`sheet-cell-${ID1}-priority`));
  });

  it("closes the peek panel", async () => {
    const parent = issue(ID1, "Parent", 1, { sub_issues_count: 1 });
    const child = { ...issue(ID2, "Child", 2, { parent_id: ID1, sub_issues_count: 1 }), state_group: null };
    const grandchild = { ...issue(ID3, "Grandchild", 3, { parent_id: ID2, sub_issues_count: 1 }), state_group: null };
    const deeper = { ...issue(ID4, "Deeper", 4, { parent_id: ID3, sub_issues_count: 1 }), state_group: null };
    stub({ pages: [page([parent])], children: { [ID1]: [child], [ID2]: [grandchild], [ID3]: [deeper] } });
    renderSheet();
    await screen.findByRole("table");
    for (const id of [ID1, ID2, ID3]) {
      await waitFor(() =>
        expect(
          document.getElementById(`issue-${id}`)?.querySelector('button[aria-label="Expand sub-work items"]')
        ).not.toBeNull()
      );
      fireEvent.click(
        document
          .getElementById(`issue-${id}`)
          ?.querySelector('button[aria-label="Expand sub-work items"]') as HTMLElement
      );
    }
    await screen.findByText("Deeper");
    fireEvent.click(
      document
        .getElementById(`issue-${ID4}`)
        ?.querySelector('button[aria-label="Expand sub-work items"]') as HTMLElement
    );
    const panel = await screen.findByTestId("issue-preview");
    expect(panel).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "Close preview" }));
    await waitFor(() => expect(screen.queryByTestId("issue-preview")).not.toBeInTheDocument());
  });
});

describe("spreadsheet row helpers", () => {
  it("builds depth-first display rows only for expanded parents", () => {
    const parent = issue(ID1, "Parent", 1);
    const child = { ...issue(ID2, "Child", 2), state_group: null };
    const grandchild = issue(ID3, "Grandchild", 3);
    const rows = buildDisplayRows(
      [parent],
      new Set([ID1, ID2]),
      new Map([
        [ID1, [child]],
        [ID2, [grandchild]],
      ])
    );
    expect(rows.map((row) => [row.issue.name, row.depth])).toEqual([
      ["Parent", 0],
      ["Child", 1],
      ["Grandchild", 2],
    ]);
    expect(buildDisplayRows([parent], new Set(), new Map())).toHaveLength(1);
    expect(buildDisplayRows([parent], new Set([ID1]), new Map())).toHaveLength(1);
  });

  it("computes a stable viewport window and reports the bottom", () => {
    const node = document.createElement("div");
    Object.defineProperty(node, "clientHeight", { value: 100 });
    Object.defineProperty(node, "scrollTop", { value: 0, writable: true, configurable: true });
    function Probe(): React.ReactElement {
      const window = useViewportWindow(node, 44, 20, 1);
      return <span data-testid="window">{JSON.stringify(window)}</span>;
    }
    render(<Probe />);
    expect(screen.getByTestId("window").textContent).toBe(JSON.stringify({ start: 0, end: 5, atBottom: false }));
  });

  it("recomputes the window on scroll and on resize", () => {
    const node = document.createElement("div");
    Object.defineProperty(node, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(node, "scrollTop", { value: 0, writable: true, configurable: true });
    const observers: Array<() => void> = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          observers.push(callback);
        }
        observe(): void {}
        disconnect(): void {}
      }
    );
    function Probe(): React.ReactElement {
      const window = useViewportWindow(node, 44, 20, 1);
      return <span data-testid="window">{JSON.stringify(window)}</span>;
    }
    const { unmount } = render(<Probe />);
    node.scrollTop = 44 * 10;
    fireEvent.scroll(node);
    expect(screen.getByTestId("window").textContent).toBe(JSON.stringify({ start: 9, end: 14, atBottom: false }));
    Object.defineProperty(node, "clientHeight", { value: 220, configurable: true });
    for (const observer of observers) act(() => observer());
    expect(screen.getByTestId("window").textContent).toBe(JSON.stringify({ start: 9, end: 16, atBottom: false }));
    unmount();
    vi.unstubAllGlobals();
  });

  it("reveals an off-window row by scrolling it into view without overshooting the end", () => {
    const node = document.createElement("div");
    Object.defineProperty(node, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(node, "scrollTop", { value: 0, writable: true, configurable: true });
    const api = { current: null as null | ReturnType<typeof useViewportWindow> };
    function Probe(): React.ReactElement {
      api.current = useViewportWindow(node, 44, 20, 1);
      return <span data-testid="window">{api.current.start}</span>;
    }
    render(<Probe />);
    act(() => api.current!.revealRow(0));
    expect(node.scrollTop).toBe(0);
    act(() => api.current!.revealRow(10));
    expect(node.scrollTop).toBe(390);
    act(() => api.current!.revealRow(19));
    expect(node.scrollTop).toBe(780);
  });

  it("does nothing when revealing a row before the scroller mounts", () => {
    const api = { current: null as null | ReturnType<typeof useViewportWindow> };
    function Probe(): React.ReactElement {
      api.current = useViewportWindow(null, 44, 20, 1);
      return <span />;
    }
    render(<Probe />);
    expect(() => api.current!.revealRow(4)).not.toThrow();
  });

  it("focuses a rendered cell and reports when the row or DOM node is unavailable", () => {
    const rows = [
      { issue: issue(ID1, "First", 1), depth: 0 },
      { issue: issue(ID2, "Second", 2), depth: 0 },
    ];
    const container = document.createElement("div");
    container.innerHTML = `<button id="issue-${ID1}"></button><button id="sheet-cell-${ID1}-state"></button>`;
    document.body.append(container);
    expect(focusSheetCell(document, rows, SHEET_COLUMNS, { row: 0, column: 0 })).toBe(true);
    expect(document.activeElement?.id).toBe(`issue-${ID1}`);
    expect(focusSheetCell(document, rows, SHEET_COLUMNS, { row: 0, column: 1 })).toBe(true);
    expect(document.activeElement?.id).toBe(`sheet-cell-${ID1}-state`);
    expect(focusSheetCell(document, rows, SHEET_COLUMNS, { row: 1, column: 1 })).toBe(false);
    expect(focusSheetCell(document, rows, SHEET_COLUMNS, { row: 5, column: 1 })).toBe(false);
  });
});

describe("SpreadsheetLayout selection", () => {
  it("selects rows from the leading cell and shows the OSS upgrade banner", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    const boxes = screen.getAllByRole("checkbox", { name: /^Select / });
    expect(boxes.length).toBeGreaterThan(0);
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
    expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument();
  });

  it("selects every row from the header control", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getByRole("checkbox", { name: "Select all work items" }));
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("2 work items selected.");
  });

  it("replaces the banner with the cloud toolbar when the edition enables bulk operations", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    expect(await screen.findByTestId("bulk-toolbar")).toHaveTextContent("1 selected");
    expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument();
  });

  it.each([
    ["Update", bulkMocks.update, { issueIds: [ID1], properties: { priority: "high" } }],
    ["Delete", bulkMocks.remove, [ID1]],
    ["Archive", bulkMocks.archive, [ID1]],
    ["Subscribe", bulkMocks.subscribe, [ID1]],
  ] as const)("runs the cloud %s action and clears on success", async (label, action, expected) => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    fireEvent.click(within(await screen.findByTestId("bulk-toolbar")).getByRole("button", { name: label }));
    await waitFor(() => expect(action).toHaveBeenCalledWith(expected));
    await waitFor(() => expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument());
  });

  it("keeps the selection when a cloud action fails", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    bulkMocks.archive.mockRejectedValueOnce(new Error("refused"));
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    fireEvent.click(within(await screen.findByTestId("bulk-toolbar")).getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(bulkMocks.archive).toHaveBeenCalled());
    expect(screen.getByTestId("bulk-toolbar")).toBeInTheDocument();
  });

  it("keeps the cloud toolbar hidden for readers without edit permission", async () => {
    edition.flags[BULK_OPERATIONS_FLAG] = true;
    stub();
    renderSheet({ canEdit: false });
    await screen.findByRole("table");
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    await waitFor(() => expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument());
    expect(screen.queryByTestId("bulk-toolbar")).not.toBeInTheDocument();
  });

  it("keeps a foreign-project row visible but out of the selection", async () => {
    const FOREIGN = "923e4567-e89b-12d3-a456-426614174009";
    stub({
      pages: [
        page([
          issue(ID1, "Fix login", 1),
          issue(FOREIGN, "Other project", 9, { project_id: "823e4567-e89b-12d3-a456-426614174008" }),
        ]),
      ],
    });
    renderSheet();
    await screen.findByRole("table");
    expect(screen.getByRole("img", { name: "Cannot select WEB-9" })).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    expect(await screen.findByTestId("bulk-upgrade-banner")).toHaveTextContent("1 work item selected.");
  });

  it("renders no selection controls when selectability is off", async () => {
    stub();
    renderSheet({ enableSelection: false });
    await screen.findByRole("table");
    expect(screen.queryByRole("checkbox", { name: /^Select / })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Select all work items" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument();
  });

  it("keeps a foreign row without a hint when selectability is off", async () => {
    const FOREIGN = "923e4567-e89b-12d3-a456-426614174009";
    stub({
      pages: [page([issue(FOREIGN, "Other project", 9, { project_id: "823e4567-e89b-12d3-a456-426614174008" })])],
    });
    renderSheet({ enableSelection: false });
    await screen.findByRole("table");
    expect(screen.queryByRole("img", { name: /Cannot select/ })).not.toBeInTheDocument();
  });

  it("clears the selection from the banner only", async () => {
    stub();
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    expect(await screen.findByTestId("bulk-upgrade-banner")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("checkbox", { name: "Select WEB-1" })[0]!);
    await waitFor(() => expect(screen.queryByTestId("bulk-upgrade-banner")).not.toBeInTheDocument());
  });

  it("marks a row for active-row scrolling and shows a partial header state", async () => {
    stub({ pages: [page([issue(ID1, "Fix login", 1), issue(ID2, "Second", 2)])] });
    renderSheet();
    await screen.findByRole("table");
    fireEvent.click(await screen.findByRole("checkbox", { name: "Select WEB-1" }));
    const header = screen.getByRole("checkbox", { name: "Select all work items" }) as HTMLInputElement;
    expect(header.indeterminate).toBe(true);
    expect(document.querySelector("[data-selection-id]")).not.toBeNull();
  });

  it("reveals an active row beyond the windowed spreadsheet slice", async () => {
    const rows = Array.from({ length: 30 }, (_, index) =>
      issue(`123e4567-e89b-12d3-a456-${String(426614174000 + index)}`, `Row ${index + 1}`, index + 1)
    );
    stub({ pages: [page(rows)] });
    renderSheet();
    await screen.findByRole("table");
    const boxes = await screen.findAllByRole("checkbox", { name: /^Select WEB-\d+$/ });
    const lastRendered = boxes.at(-1)!;
    const lastSequence = Number(lastRendered.getAttribute("aria-label")!.replace("Select WEB-", ""));
    const next = rows[lastSequence];
    fireEvent.click(lastRendered);
    expect(document.querySelector(`[data-selection-id="${next!.id}"]`)).toBeNull();
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
    });
    const scroller = screen.getByTestId("spreadsheet-scroller");
    expect(scroller.scrollTop).toBeGreaterThan(0);
    await waitFor(() =>
      expect(document.querySelector(`[data-selection-id="${next!.id}"]`)).toHaveAttribute("data-active", "true")
    );
  });
});
