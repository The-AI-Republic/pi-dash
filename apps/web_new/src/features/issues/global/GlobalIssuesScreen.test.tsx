// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { GlobalIssueRow } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/api/keys.js";
import { GLOBAL_LOAD_MORE_LABEL, GlobalIssuesScreen, globalIssueKey, rowPeekSelection } from "./GlobalIssuesScreen.js";
import { globalIssuesQueryOptions } from "./queries.js";

// The peek composition is its own suite's subject; this list owns only
// the address it hands over, so the stub echoes what it received and
// exposes the panel's reports.
const peekHarness = vi.hoisted(() => ({
  props: null as null | {
    peeked: string;
    peekProjectId?: string | null;
    projectId: string;
    projectIdentifier: string;
    onPeekChange?: (selection: unknown) => void;
    onClose: () => void;
  },
  reports: [] as unknown[],
  closes: 0,
}));
vi.mock("../peek/ResolvingIssuePeek.js", () => ({
  ResolvingIssuePeek: (props: {
    peeked: string;
    peekProjectId?: string | null;
    projectId: string;
    projectIdentifier: string;
    onPeekChange?: (selection: unknown) => void;
    onClose: () => void;
  }) => {
    peekHarness.props = props;
    return (
      <div
        aria-label="Work item preview"
        data-peeked={props.peeked}
        data-peek-project={props.peekProjectId ?? ""}
        data-project={props.projectId}
        data-identifier={props.projectIdentifier}
      >
        <button
          type="button"
          onClick={() =>
            props.onPeekChange?.({
              issueId: "923e4567-e89b-12d3-a456-426614174013",
              projectId: OTHER_PROJECT,
              nestingLevel: 1,
            })
          }
        >
          Open child
        </button>
        <button type="button" onClick={props.onClose}>
          Close preview
        </button>
      </div>
    );
  },
}));

// The empty surface reads workspace standing for its entries; the list
// rows do not, so the harness pins a member throughout.
vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  usePermissions: () => ({ role: 20, isMember: true, canEdit: true, isAdmin: false, isLoading: false }),
}));

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "323e4567-e89b-12d3-a456-426614174002";

function row(seed: string, overrides: Partial<GlobalIssueRow> = {}): GlobalIssueRow {
  const id = idFor(seed);
  return {
    id,
    name: `Issue ${seed}`,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    sequence_id: 1,
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
    state__group: null,
    ...overrides,
  };
}

/** A stable UUID for a short test seed, so the row schema accepts it. */
function idFor(seed: string): string {
  let hex = "";
  for (const char of seed) hex += char.charCodeAt(0).toString(16).padStart(2, "0");
  const filled = `${hex}${"0".repeat(40)}`.slice(0, 32);
  return `${filled.slice(0, 8)}-${filled.slice(8, 12)}-4${filled.slice(13, 16)}-8${filled.slice(17, 20)}-${filled.slice(20, 32)}`;
}

function projectRow(id: string, identifier: string, memberRole = 15): Record<string, unknown> {
  return { id, name: identifier, identifier, member_role: memberRole, archived_at: null };
}

/** One response page, with the cursor fields the read expects. */
function page(results: GlobalIssueRow[], options: { total?: number; nextCursor?: string } = {}) {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: options.total ?? results.length,
    next_cursor: options.nextCursor ?? "",
    prev_cursor: "",
    next_page_results: options.nextCursor !== undefined && options.nextCursor !== "",
    prev_page_results: false,
    count: results.length,
    total_results: options.total ?? results.length,
    results,
  };
}

function renderScreen(
  issues: { results: GlobalIssueRow[]; total_count?: number },
  routeFilters: Record<string, string> = {},
  options: {
    withoutWorkspaceParam?: boolean;
    /** The continuation the transport answers for a cursor request. */
    nextPage?: ReturnType<typeof page>;
    /** Records each read URL, for proving the cursor round-trip. */
    onFetch?: (url: string) => void;
    /** Visible projects; the empty surface picks its card from them. */
    projects?: Array<Record<string, unknown>>;
    /** Props for an embedding view: scoped read, hidden header, empty override, label. */
    screenProps?: React.ComponentProps<typeof GlobalIssuesScreen>;
  } = {}
) {
  createAppClient("https://api.test", ((url: string) => {
    options.onFetch?.(url);
    const body = url.includes("cursor=") ? (networkPages?.next ?? null) : (networkPages?.first ?? null);
    if (body === undefined || body === null) return Promise.reject(new Error("unexpected network"));
    return Promise.resolve({
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    });
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  const firstPage = page(issues.results, {
    ...(issues.total_count === undefined ? {} : { total: issues.total_count }),
    ...(options.nextPage === undefined ? {} : { nextCursor: "page-2" }),
  });
  networkPages = { first: firstPage, ...(options.nextPage === undefined ? {} : { next: options.nextPage }) };
  queryClient.setQueryData(
    globalIssuesQueryOptions(
      getAppClient(),
      options.withoutWorkspaceParam ? "" : SLUG,
      options.screenProps?.query ?? {}
    ).queryKey,
    { pages: [firstPage], pageParams: [null] }
  );
  queryClient.setQueryData(
    projectKeys.list(options.withoutWorkspaceParam ? "" : SLUG),
    options.projects ?? [projectRow(PROJECT, "WEB"), projectRow(OTHER_PROJECT, "API")]
  );
  peekHarness.props = null;
  peekHarness.reports = [];
  peekHarness.closes = 0;
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const layout = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const browse = createRoute({
    getParentRoute: () => layout,
    path: "/browse/$issueKey",
    component: () => <div data-testid="browse" />,
  });
  const allIssuesParent = options.withoutWorkspaceParam ? rootRoute : layout;
  const allIssuesPath = options.withoutWorkspaceParam ? "/workspace-views/all-issues" : "/workspace-views/all-issues";
  const allIssues = createRoute({
    getParentRoute: () => allIssuesParent,
    path: allIssuesPath,
    validateSearch: (search: Record<string, unknown>) => search as Record<string, string>,
    component: () => <GlobalIssuesScreen {...(options.screenProps ?? {})} />,
  });
  const query = new URLSearchParams(routeFilters).toString();
  const router = createRouter({
    routeTree: options.withoutWorkspaceParam
      ? rootRoute.addChildren([layout.addChildren([browse]), allIssues])
      : rootRoute.addChildren([layout.addChildren([browse, allIssues])]),
    context: { queryClient },
    history: createMemoryHistory({
      initialEntries: [
        options.withoutWorkspaceParam
          ? `/workspace-views/all-issues${query ? `?${query}` : ""}`
          : `/${SLUG}/workspace-views/all-issues${query ? `?${query}` : ""}`,
      ],
    }),
  });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { view, queryClient, router };
}

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

// jsdom reports zero sizes, so the virtual list would compute an empty
// window. Give the scroll region and rows a height so rows mount.
const realHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");

/** Every sentinel observer mounted this test, so a test can scroll it. */
const capturedObservers: Array<{ trigger(): void; disconnect(): void }> = [];

beforeEach(() => {
  // The address is shared by every case in the file; start each from the
  // plain view so a peek written by an earlier case cannot seed a link.
  window.history.replaceState({}, "", `/${SLUG}/workspace-views/all-issues`);
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region") return 400;
      if (this.hasAttribute("data-index")) return 40;
      return realHeight?.get?.call(this) ?? 0;
    },
  });
  capturedObservers.length = 0;
  class TestIntersectionObserver {
    private readonly callback: IntersectionObserverCallback;
    constructor(callback: IntersectionObserverCallback) {
      this.callback = callback;
      capturedObservers.push(this);
    }
    observe(): void {}
    disconnect(): void {}
    /** Drive the sentinel as the browser would when it scrolls into view. */
    trigger(): void {
      this.callback([{ isIntersecting: true } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
  }
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
});

// The infinite-query mount refetches its first window; answer with the
// seeded first page for the plain read and the seeded second page for a
// cursor continuation.
let networkPages: { first: unknown; next?: unknown } | null = null;
afterEach(() => {
  networkPages = null;
});

describe("globalIssueKey", () => {
  it("prefixes the sequence with the project identifier", () => {
    expect(globalIssueKey("WEB", { sequence_id: 42 })).toBe("WEB-42");
    expect(globalIssueKey(undefined, { sequence_id: 7 })).toBe("?-7");
  });
});

describe("rowPeekSelection", () => {
  it("opens the row's own id at the top of the stack", () => {
    expect(rowPeekSelection({ id: "issue-1", project_id: PROJECT })).toEqual({
      issueId: "issue-1",
      projectId: PROJECT,
      nestingLevel: 0,
    });
  });
});

describe("GlobalIssuesScreen", () => {
  it("lists rows from every project with their short keys", async () => {
    renderScreen({ results: [row("a1"), row("b2", { project_id: OTHER_PROJECT, sequence_id: 9 })] });
    expect(await screen.findByText("Issue a1")).toBeInTheDocument();
    expect(screen.getByText("Issue b2")).toBeInTheDocument();
    expect(screen.getByText("WEB-1")).toBeInTheDocument();
    expect(screen.getByText("API-9")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("renders the same unfiltered rows when URL filters are present (ISS-223)", async () => {
    renderScreen({ results: [row("a1"), row("b2")] }, { priority: "urgent", order_by: "-created_at" });
    expect(await screen.findByText("Issue a1")).toBeInTheDocument();
    expect(screen.getByText("Issue b2")).toBeInTheDocument();
  });

  it("renders the workspace card when the read returns no rows (ISS-073)", async () => {
    renderScreen({ results: [] });
    expect(await screen.findByText("No Views yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add work item" })).toBeEnabled();
  });

  it("asks for a project when the workspace shows none (ISS-073)", async () => {
    renderScreen({ results: [] }, {}, { projects: [] });
    expect(await screen.findByText("No project")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start your first project" })).toBeEnabled();
  });

  it("shows the same card when URL filters are present because the list ignores them (ISS-223)", async () => {
    renderScreen({ results: [] }, { priority: "urgent" });
    expect(await screen.findByText("No Views yet")).toBeInTheDocument();
  });

  it("falls back to an empty slug when the route carries no workspace param", async () => {
    renderScreen({ results: [row("a1", { sequence_id: 3 })] }, {}, { withoutWorkspaceParam: true });
    const link = await screen.findByRole("link", { name: /WEB-3/ });
    expect(link.getAttribute("href")).toBe("/browse/WEB-3");
  });

  it("links a row to the workspace browse route", async () => {
    const { queryClient } = renderScreen({ results: [row("a1", { sequence_id: 5 })] });
    const link = await screen.findByRole("link", { name: /WEB-5/ });
    expect(link.getAttribute("href")).toBe(`/${SLUG}/browse/WEB-5`);
    queryClient.clear();
  });

  it("appends the next cursor page instead of stopping at the first window (ISS-223)", async () => {
    const first = Array.from({ length: 100 }, (_unused, index) => row(`p1-${index}`, { sequence_id: index + 1 }));
    const second = Array.from({ length: 20 }, (_unused, index) => row(`p2-${index}`, { sequence_id: 101 + index }));
    const seen: string[] = [];
    const { queryClient } = renderScreen(
      { results: first, total_count: 120 },
      {},
      { nextPage: page(second, { total: 120 }), onFetch: (url) => seen.push(url) }
    );
    // The header reports the server total while the first window mounts.
    expect(await screen.findByText("120")).toBeInTheDocument();
    const more = await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await userEvent.setup().click(more);
    // The virtual list mounts only the visible window, so the appended
    // page is proven through the cached rows and the retired control.
    await waitFor(() =>
      expect(
        queryClient.getQueryData<{ pages: unknown[] }>(globalIssuesQueryOptions(getAppClient(), SLUG).queryKey)
      ).toHaveProperty("pages.1.results.19.id", idFor("p2-19"))
    );
    await waitFor(() => expect(screen.queryByRole("button", { name: GLOBAL_LOAD_MORE_LABEL })).not.toBeInTheDocument());
  });

  it("sends the cursor to the server when the reader asks for more", async () => {
    const first = [row("p1-0", { sequence_id: 1 })];
    const seen: string[] = [];
    renderScreen(
      { results: first, total_count: 2 },
      {},
      {
        nextPage: page([row("p2-0", { sequence_id: 2 })], { total: 2 }),
        onFetch: (url) => seen.push(url),
      }
    );
    expect((await screen.findAllByText("Issue p1-0")).length).toBeGreaterThan(0);
    const more = await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await userEvent.setup().click(more);
    expect(seen.some((url) => url.includes("cursor=page-2"))).toBe(true);
  });

  it("asks for the next page when the sentinel scrolls into view", async () => {
    const first = [row("p1-0", { sequence_id: 1 })];
    const seen: string[] = [];
    renderScreen(
      { results: first, total_count: 2 },
      {},
      { nextPage: page([row("p2-0", { sequence_id: 2 })], { total: 2 }), onFetch: (url) => seen.push(url) }
    );
    await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await waitFor(() => expect(capturedObservers.length).toBeGreaterThan(0));
    act(() => capturedObservers.at(-1)?.trigger());
    await waitFor(() => expect(seen.some((url) => url.includes("cursor=page-2"))).toBe(true));
  });

  it("asks for one continuation at a time while a page is in flight", async () => {
    const first = [row("p1-0", { sequence_id: 1 })];
    const seen: string[] = [];
    renderScreen(
      { results: first, total_count: 3 },
      {},
      {
        nextPage: page([row("p2-0", { sequence_id: 2 })], { total: 3, nextCursor: "page-3" }),
        onFetch: (url) => seen.push(url),
      }
    );
    await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await waitFor(() => expect(capturedObservers.length).toBeGreaterThan(0));
    const observer = capturedObservers.at(-1);
    act(() => observer?.trigger());
    act(() => observer?.trigger());
    await waitFor(() => expect(seen.filter((url) => url.includes("cursor=page-2")).length).toBe(1));
  });

  it("does not read again once the chain ended", async () => {
    renderScreen({ results: [row("only", { sequence_id: 1 })] });
    await screen.findByText("Issue only");
    act(() => capturedObservers.forEach((observer) => observer.trigger()));
    // With no continuation there is no footer sentinel at all, so the
    // observer list stays empty and nothing fires.
    expect(capturedObservers).toHaveLength(0);
  });

  it("gives every row its project-style menu and the page its ellipsis (ISS-065/067)", async () => {
    const user = userEvent.setup();
    renderScreen({ results: [row("a1", { sequence_id: 1, state__group: "started" })] });
    await screen.findByText("Issue a1");
    await user.click(screen.getAllByRole("button", { name: "Actions for WEB-1" })[0]!);
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.querySelector("[data-menu-title]")?.textContent ?? item.textContent)).toEqual([
      "Edit",
      "Make a copy",
      "Open in new tab",
      "Copy link",
      "Move to project",
      "Archive",
      "Delete",
    ]);
    // A started row cannot be archived: the note explains the gate.
    expect(items[5]?.textContent).toContain("Only completed or canceled");
    expect(items[5]?.getAttribute("aria-disabled")).toBe("true");

    expect(screen.getByRole("button", { name: "List actions" })).toBeInTheDocument();
  });

  it("hides the write entries on a project the viewer only reads", async () => {
    const user = userEvent.setup();
    const view = renderScreen({ results: [row("a1", { sequence_id: 1 })] });
    // A guest standing on the row's own project hides every write entry.
    view.queryClient.setQueryData(projectKeys.list(SLUG), [projectRow(PROJECT, "WEB", 10)]);
    await screen.findByText("Issue a1");
    await user.click(screen.getAllByRole("button", { name: "Actions for WEB-1" })[0]!);
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.querySelector("[data-menu-title]")?.textContent ?? item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
    ]);
  });

  it("keys a row from an unknown project by '?' with a read-only menu (ISS-065)", async () => {
    const user = userEvent.setup();
    renderScreen({ results: [row("zz", { sequence_id: 7, project_id: idFor("unknown") })] });
    await screen.findByText("Issue zz");
    expect(screen.getByText("?-7")).toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: "Actions for ?-7" })[0]!);
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.querySelector("[data-menu-title]")?.textContent ?? item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
    ]);
  });

  it("scopes the read to the embedding view's expression and order", async () => {
    const seen: string[] = [];
    renderScreen(
      { results: [row("scoped", { sequence_id: 1 })], total_count: 2 },
      {},
      {
        nextPage: page([row("scoped-next", { sequence_id: 2 })], { total: 2 }),
        onFetch: (url) => seen.push(url),
        screenProps: { query: { orderBy: "-created_at", filters: { assignees: "me" } } },
      }
    );
    expect(await screen.findByText("Issue scoped")).toBeInTheDocument();
    const more = await screen.findByRole("button", { name: GLOBAL_LOAD_MORE_LABEL });
    await userEvent.setup().click(more);
    await waitFor(() => expect(seen.some((url) => url.includes("cursor=page-2"))).toBe(true));
    const continuation = seen.find((url) => url.includes("cursor=page-2")) ?? "";
    expect(continuation).toContain("order_by=-created_at");
    expect(decodeURIComponent(continuation)).toContain("assignees=me");
  });

  it("hides its header and labels the list for the embedding view", async () => {
    renderScreen(
      { results: [row("a1", { sequence_id: 1 })] },
      {},
      { screenProps: { hideHeader: true, label: "Triage" } }
    );
    // The section and the virtual list share the view's label.
    expect(await screen.findAllByRole("region", { name: "Triage" })).toHaveLength(2);
    expect(screen.queryByRole("heading")).toBeNull();
    expect(screen.getByText("Issue a1")).toBeInTheDocument();
  });

  it("renders the empty override instead of the workspace card", async () => {
    renderScreen(
      { results: [] },
      {},
      { screenProps: { emptyState: <p data-testid="view-empty-override">Nothing matches this view.</p> } }
    );
    expect(await screen.findByTestId("view-empty-override")).toBeInTheDocument();
    expect(screen.queryByText("No Views yet")).toBeNull();
  });
});

describe("GlobalIssuesScreen peek address (ISS-165/167)", () => {
  const FIRST = idFor("a1");

  /** The window address a standalone mount writes through `replaceState`. */
  function lastWindowSearch(): URLSearchParams {
    const spy = window.history.replaceState as unknown as { mock?: { calls: unknown[][] } };
    const calls = spy.mock?.calls ?? [];
    const url = String(calls.at(-1)?.[2] ?? "");
    return new URLSearchParams(url.includes("?") ? url.slice(url.indexOf("?") + 1) : "");
  }

  it("opens the peek on a plain row click, writing replace with the row's project", async () => {
    const replace = vi.spyOn(window.history, "replaceState");
    const user = userEvent.setup();
    renderScreen({ results: [row("a1", { sequence_id: 1, project_id: OTHER_PROJECT })] });
    await user.click(await screen.findByRole("link", { name: /API-1/ }));
    expect(await screen.findByLabelText("Work item preview")).toHaveAttribute("data-peeked", FIRST);
    expect(replace).toHaveBeenCalled();
    const address = lastWindowSearch();
    expect(address.get("peekIssueId")).toBe(FIRST);
    // The surface has no route project, so the row's own is always named.
    expect(address.get("peekProjectId")).toBe(OTHER_PROJECT);
    expect(address.get("peekNestingLevel")).toBeNull();
    replace.mockRestore();
  });

  it("keeps the full page for a modified click", async () => {
    renderScreen({ results: [row("a1", { sequence_id: 1 })] });
    const link = await screen.findByRole("link", { name: /WEB-1/ });
    for (const init of [{ metaKey: true }, { ctrlKey: true }, { shiftKey: true }]) {
      fireEvent.click(link, init);
      expect(screen.queryByLabelText("Work item preview"), JSON.stringify(init)).toBeNull();
    }
  });

  it("opens a deep link without a row ever being clicked (ISS-167)", async () => {
    renderScreen(
      { results: [row("a1", { sequence_id: 1 })] },
      {},
      { screenProps: { peekKey: FIRST, peekProjectId: PROJECT } }
    );
    const panel = await screen.findByLabelText("Work item preview");
    expect(panel).toHaveAttribute("data-peeked", FIRST);
    expect(panel).toHaveAttribute("data-peek-project", PROJECT);
    // A held row supplies the project and identifier the panel resolves by.
    expect(panel).toHaveAttribute("data-project", PROJECT);
    expect(panel).toHaveAttribute("data-identifier", "WEB");
  });

  it("stays closed for an address that names no project and no visible row", async () => {
    // The workspace route has no path project, so an id the loaded rows do
    // not own has nothing to read under and stays closed.
    renderScreen({ results: [row("a1", { sequence_id: 1 })] }, {}, { screenProps: { peekKey: idFor("elsewhere") } });
    expect(await screen.findByText("Issue a1")).toBeInTheDocument();
    expect(screen.queryByLabelText("Work item preview")).toBeNull();
  });

  it("resolves a deep link that names only an id from a loaded row", async () => {
    renderScreen({ results: [row("a1", { sequence_id: 1 })] }, {}, { screenProps: { peekKey: FIRST } });
    const panel = await screen.findByLabelText("Work item preview");
    expect(panel).toHaveAttribute("data-peeked", FIRST);
    // The held row supplies the project; the address names no override.
    expect(panel).toHaveAttribute("data-project", PROJECT);
    expect(panel).toHaveAttribute("data-identifier", "WEB");
  });

  it("falls back to no identifier when the peeked project is not in the loaded list", async () => {
    renderScreen(
      { results: [row("a1", { sequence_id: 1 })] },
      {},
      { screenProps: { peekKey: FIRST, peekProjectId: idFor("hidden") } }
    );
    const panel = await screen.findByLabelText("Work item preview");
    expect(panel).toHaveAttribute("data-project", idFor("hidden"));
    expect(panel).toHaveAttribute("data-identifier", "");
  });

  it("mounts the deep-linked peek even before any row is visible", async () => {
    renderScreen({ results: [] }, {}, { screenProps: { peekKey: FIRST, peekProjectId: OTHER_PROJECT } });
    // The empty card still renders; the panel stands beside it.
    expect(await screen.findByText("No Views yet")).toBeInTheDocument();
    expect(await screen.findByLabelText("Work item preview")).toHaveAttribute("data-peeked", FIRST);
  });

  it("closes through the address and strips every peek param", async () => {
    const replace = vi.spyOn(window.history, "replaceState");
    const user = userEvent.setup();
    // The standalone mount writes into the live address, so seed the
    // parameters that must survive a close.
    window.history.replaceState({}, "", `/${SLUG}/workspace-views/all-issues?layout=list&order_by=-created_at`);
    renderScreen({ results: [row("a1", { sequence_id: 1 })] });
    await user.click(await screen.findByRole("link", { name: /WEB-1/ }));
    await user.click(await screen.findByRole("button", { name: "Close preview" }));
    expect(screen.queryByLabelText("Work item preview")).toBeNull();
    const address = lastWindowSearch();
    expect(address.get("peekIssueId")).toBeNull();
    expect(address.get("peekProjectId")).toBeNull();
    expect(address.get("peekNestingLevel")).toBeNull();
    // The rest of the address survives the strip.
    expect(address.get("layout")).toBe("list");
    expect(address.get("order_by")).toBe("-created_at");
    replace.mockRestore();
  });

  it("reports a nested open through the address and keeps the parent panel", async () => {
    const replace = vi.spyOn(window.history, "replaceState");
    const user = userEvent.setup();
    renderScreen({ results: [row("a1", { sequence_id: 1 })] });
    await user.click(await screen.findByRole("link", { name: /WEB-1/ }));
    await user.click(await screen.findByRole("button", { name: "Open child" }));
    const nested = lastWindowSearch();
    expect(nested.get("peekIssueId")).toBe("923e4567-e89b-12d3-a456-426614174013");
    expect(nested.get("peekProjectId")).toBe(OTHER_PROJECT);
    expect(nested.get("peekNestingLevel")).toBe("1");
    // The panel the layout opened stays mounted under the child.
    expect(screen.getByLabelText("Work item preview")).toHaveAttribute("data-peeked", FIRST);
    replace.mockRestore();
  });

  it("forwards reports to a host's writer instead of keeping them (ISS-167)", async () => {
    const writer = vi.fn();
    const user = userEvent.setup();
    renderScreen(
      { results: [row("a1", { sequence_id: 1 })] },
      {},
      { screenProps: { peekKey: null, onPeekChange: writer } }
    );
    await user.click(await screen.findByRole("link", { name: /WEB-1/ }));
    expect(writer).toHaveBeenLastCalledWith({ issueId: FIRST, projectId: PROJECT, nestingLevel: 0 });
    // Controlled: the host decides what mounts, so this list keeps none.
    expect(screen.queryByLabelText("Work item preview")).toBeNull();
  });
});
