// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-cycles tab: a flat archived list with no group
// headings, the skeleton, zero, no-match and error states, the
// expandable search and date-only filters narrowing client-side, the
// address-driven peek, and the shared row menu's restore entry that
// stays on the tab.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type AnyRouter,
} from "@tanstack/react-router";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ArchivedCycleDetail, ArchivedCycleListItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  CYCLE_RESTORED_LINE,
  CYCLE_RESTORED_TITLE,
  CYCLE_RESTORE_FAILED_LINE,
  CYCLE_RESTORE_FAILED_TITLE,
  resetCycleFilterStore,
  resetCycleSearchStore,
} from "../../cycles/index.js";
import { CYCLES_PROJECT_ID, jsonResponse, makeProjectRow } from "../../cycles/testing.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { ArchivedCyclesScreen, type ArchivedCyclesScreenProps } from "./ArchivedCyclesScreen.js";

const FIRST_ID = "323e4567-e89b-12d3-a456-426614174101";
const SECOND_ID = "323e4567-e89b-12d3-a456-426614174102";
const CREATOR_ID = "123e4567-e89b-12d3-a456-426614174010";

const LIST_PATH = `/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/archived-cycles/`;
const PROJECT_PATH = `/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/`;
const MEMBERS_PATH = "/api/workspaces/acme/members/";
const TAB_PATH = `/acme/projects/${CYCLES_PROJECT_ID}/archives/cycles`;

interface Harness {
  cycles: ArchivedCycleListItem[];
  listStatus: number;
  holdList: boolean;
  releaseList: (() => void) | null;
  holdProject: boolean;
  releaseProject: (() => void) | null;
  roles: Record<string, number>;
  members: unknown[];
  restores: number;
  restoreStatus: number;
}

let harness: Harness;
let router: AnyRouter;

function archivedRow(overrides: Partial<ArchivedCycleListItem> = {}): ArchivedCycleListItem {
  return {
    id: FIRST_ID,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: CYCLES_PROJECT_ID,
    name: "Archived alpha",
    description: "",
    start_date: "2026-01-01T00:00:00Z",
    end_date: "2026-02-01T00:00:00Z",
    owned_by_id: CREATOR_ID,
    status: "COMPLETED",
    archived_at: "2026-03-01T00:00:00Z",
    is_favorite: false,
    total_issues: 2,
    completed_issues: 2,
    started_issues: 0,
    unstarted_issues: 0,
    cancelled_issues: 0,
    backlog_issues: 0,
    assignee_ids: [],
    ...overrides,
  };
}

function archivedDetail(row: ArchivedCycleListItem): ArchivedCycleDetail {
  return {
    ...row,
    sub_issues: 0,
    completed_estimate_points: 0,
    total_estimate_points: 0,
    estimate_distribution: {},
    distribution: { assignees: [], labels: [], completion_chart: null },
  };
}

function seedCycles(): ArchivedCycleListItem[] {
  return [archivedRow({ id: FIRST_ID, name: "Archived alpha" }), archivedRow({ id: SECOND_ID, name: "Archived beta" })];
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  harness = {
    cycles: seedCycles(),
    listStatus: 200,
    holdList: false,
    releaseList: null,
    holdProject: false,
    releaseProject: null,
    roles: { [CYCLES_PROJECT_ID]: 20 },
    members: [],
    restores: 0,
    restoreStatus: 204,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (method === "DELETE" && url.includes("/cycles/") && url.endsWith("/archive/")) {
      harness.restores += 1;
      if (harness.restoreStatus === 204) {
        harness.cycles = harness.cycles.filter((cycle) => !url.includes(`/cycles/${cycle.id}/archive/`));
        return jsonResponse(url, 204, null);
      }
      return jsonResponse(url, harness.restoreStatus, {});
    }
    if (method === "GET" && url.endsWith(LIST_PATH)) {
      if (harness.holdList) {
        await new Promise<void>((resolve) => {
          harness.releaseList = resolve;
        });
      }
      return jsonResponse(url, harness.listStatus, harness.cycles);
    }
    if (method === "GET" && url.includes(LIST_PATH)) {
      const row = harness.cycles.find((cycle) => url.includes(`/archived-cycles/${cycle.id}/`));
      if (row === undefined) return jsonResponse(url, 404, { detail: "missing" });
      return jsonResponse(url, 200, archivedDetail(row));
    }
    if (url.endsWith(PROJECT_PATH)) {
      if (harness.holdProject) {
        await new Promise<void>((resolve) => {
          harness.releaseProject = resolve;
        });
      }
      return jsonResponse(url, 200, makeProjectRow({ timezone: "UTC" }));
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith(MEMBERS_PATH)) return jsonResponse(url, 200, harness.members);
    throw new Error(`unexpected request ${method} ${url}`);
  }) as never);
  Object.defineProperty(window, "scrollTo", { value: () => undefined, writable: true });
});

afterEach(() => {
  resetAppClient();
  resetCycleFilterStore();
  resetCycleSearchStore();
  vi.clearAllMocks();
});

function renderScreen(props: Partial<ArchivedCyclesScreenProps> = {}): void {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const tabRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: TAB_PATH,
    component: () => (
      <ArchivedCyclesScreen
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        peekCycleId={null}
        onPeekChange={() => undefined}
        {...props}
      />
    ),
  });
  router = createRouter({
    routeTree: rootRoute.addChildren([tabRoute]),
    history: createMemoryHistory({ initialEntries: [TAB_PATH] }),
    defaultPreload: false,
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

function rowNames(): string[] {
  return screen
    .getAllByTestId("cycle-row")
    .map((row) =>
      within(row)
        .getByRole("button", { name: /Cycle actions for / })
        .getAttribute("aria-label")
    )
    .map((label) => (label ?? "").replace(/^Cycle actions for /, ""));
}

// Menu clicks go through fireEvent: user-event's pointer sequence trips
// Base UI's mouseup guard under jsdom (see kit Menu.test.tsx).
async function openMenu(cycleName: string): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: `Cycle actions for ${cycleName}` }));
  await screen.findAllByRole("menuitem");
}

function menuTitles(): string[] {
  return screen.getAllByRole("menuitem").map((item) => within(item).getByRole("heading").textContent ?? "");
}

function pickEntry(title: string): void {
  const items = screen.getAllByRole("menuitem");
  const match = items.find((item) => within(item).queryByRole("heading", { name: title }) !== null);
  if (match === undefined) throw new Error(`menu entry missing: ${title}`);
  fireEvent.click(match);
}

describe("ArchivedCyclesScreen", () => {
  it("skeletons first, then lists archived rows flat with no group headings", async () => {
    harness.holdList = true;
    renderScreen();

    expect(await screen.findByTestId("archived-cycles-screen")).toBeInTheDocument();
    expect(screen.getByTestId("archived-cycles-skeleton")).toBeInTheDocument();
    harness.holdList = false;
    harness.releaseList?.();
    expect(await screen.findByText("Archived alpha")).toBeInTheDocument();
    expect(rowNames()).toEqual(["Archived alpha", "Archived beta"]);
    expect(screen.queryByTestId("archived-cycles-skeleton")).not.toBeInTheDocument();
    expect(document.querySelectorAll('section[data-testid^="cycle-group-"]')).toHaveLength(0);
    expect(screen.getByTestId("archived-cycles-search-toggle")).toBeInTheDocument();
    expect(screen.getByTestId("cycles-filters-trigger")).toBeInTheDocument();
    expect(screen.getByTestId("archived-cycles-header-spacer")).toBeInTheDocument();
  });

  it("renders rows while the project read is still in flight", async () => {
    harness.holdProject = true;
    renderScreen();

    expect(await screen.findByText("Archived alpha")).toBeInTheDocument();
    expect(rowNames()).toEqual(["Archived alpha", "Archived beta"]);
    harness.holdProject = false;
    harness.releaseProject?.();
  });

  it("renders the zero state when the archive holds nothing", async () => {
    harness.cycles = [];
    renderScreen();

    expect(await screen.findByRole("heading", { name: "No archived cycles yet" })).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-row")).not.toBeInTheDocument();
  });

  it("retries a failed read instead of a stuck skeleton", async () => {
    harness.listStatus = 500;
    renderScreen();

    expect(await screen.findByTestId("cycles-load-error")).toBeInTheDocument();
    harness.listStatus = 200;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText("Archived alpha")).toBeInTheDocument();
  });

  it("filters the list live from the search box and clears from the no-match view", async () => {
    renderScreen();
    await screen.findByText("Archived alpha");

    fireEvent.click(screen.getByTestId("archived-cycles-search-toggle"));
    fireEvent.change(screen.getByTestId("archived-cycles-search-input"), { target: { value: "zzz-no-such-cycle" } });

    const view = await screen.findByTestId("cycles-no-match");
    expect(view).toHaveTextContent(/search/i);
    expect(screen.queryByTestId("cycle-row")).not.toBeInTheDocument();
    fireEvent.click(within(view).getByRole("button", { name: "Clear search" }));

    expect(await screen.findByText("Archived alpha")).toBeInTheDocument();
    expect(rowNames()).toHaveLength(2);
  });

  it("offers start and due dates without status, chips the pick, and removes it", async () => {
    renderScreen();
    await screen.findByText("Archived alpha");

    fireEvent.click(screen.getByTestId("cycles-filters-trigger"));
    const panel = screen.getByTestId("cycles-filters-panel");
    const sections = Array.from(panel.querySelectorAll("[data-section-title]")).map((section) =>
      section.getAttribute("data-section-title")
    );
    expect(sections).toEqual(["Start date", "Due date"]);
    fireEvent.click(
      within(screen.getByTestId("cycle-filter-start-date")).getByRole("checkbox", { name: "1 week from now" })
    );

    const chips = await screen.findByTestId("cycles-filter-chips");
    expect(chips).toHaveTextContent("Start date");
    expect(chips).toHaveTextContent("1 week from now");
    expect(screen.getByTestId("cycles-filters-count")).toBeInTheDocument();
    expect(await screen.findByTestId("cycles-no-match")).toHaveTextContent(/filter/i);
    expect(screen.queryByTestId("cycle-row")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Remove 1 week from now" }));
    expect(await screen.findByText("Archived alpha")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-filter-chips")).not.toBeInTheDocument();
  });

  it("drops every chip at once through clear-all", async () => {
    renderScreen();
    await screen.findByText("Archived alpha");

    fireEvent.click(screen.getByTestId("cycles-filters-trigger"));
    fireEvent.click(
      within(screen.getByTestId("cycle-filter-start-date")).getByRole("checkbox", { name: "1 week from now" })
    );
    fireEvent.click(
      within(screen.getByTestId("cycle-filter-due-date")).getByRole("checkbox", { name: "1 week from now" })
    );
    await screen.findByTestId("cycles-filter-chips");
    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));

    expect(await screen.findByText("Archived alpha")).toBeInTheDocument();
    expect(screen.queryByTestId("cycles-filter-chips")).not.toBeInTheDocument();
  });

  it("peeks a row's archived record and closes back to the list", async () => {
    const onPeekChange = vi.fn();
    renderScreen({ onPeekChange });
    await screen.findByText("Archived alpha");

    fireEvent.click(screen.getByRole("button", { name: "Quick look at Archived alpha" }));
    expect(onPeekChange).toHaveBeenCalledWith(FIRST_ID);
  });

  it("toggles the peek shut from its own row", async () => {
    const onPeekChange = vi.fn();
    renderScreen({ onPeekChange, peekCycleId: FIRST_ID });
    await screen.findByText("Archived alpha");
    await screen.findByTestId("archived-cycle-preview");

    fireEvent.click(screen.getByRole("button", { name: "Quick look at Archived alpha" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("renders the peek body from the archived detail read", async () => {
    renderScreen({ peekCycleId: FIRST_ID });
    await screen.findByText("Archived alpha");

    const panel = await screen.findByTestId("archived-cycle-preview");
    expect(within(panel).getByRole("heading", { name: "Archived alpha" })).toBeInTheDocument();
    expect(await screen.findByTestId("archived-cycle-peek-body")).toHaveTextContent("2 of 2 work items");
  });

  it("closes the peek through its dismiss control", async () => {
    const onPeekChange = vi.fn();
    renderScreen({ onPeekChange, peekCycleId: FIRST_ID });
    await screen.findByTestId("archived-cycle-preview");

    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("offers restore next to the link actions on an archived row", async () => {
    renderScreen();
    await screen.findByText("Archived alpha");

    await openMenu("Archived alpha");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link", "Restore"]);
  });

  it("restores from the menu and stays on the tab", async () => {
    renderScreen();
    await screen.findByText("Archived alpha");

    await openMenu("Archived alpha");
    pickEntry("Restore");

    await vi.waitFor(() => expect(harness.restores).toBe(1));
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_RESTORED_TITLE, CYCLE_RESTORED_LINE);
    await vi.waitFor(() => expect(screen.queryByText("Archived alpha")).not.toBeInTheDocument());
    expect(screen.getByText("Archived beta")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe(TAB_PATH);
  });

  it("reports a failed restore and keeps the row archived", async () => {
    harness.restoreStatus = 404;
    renderScreen();
    await screen.findByText("Archived alpha");

    await openMenu("Archived alpha");
    pickEntry("Restore");

    await vi.waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_RESTORE_FAILED_TITLE, CYCLE_RESTORE_FAILED_LINE)
    );
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(screen.getByText("Archived alpha")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe(TAB_PATH);
  });

  it("shows guests the link actions without restore", async () => {
    harness.roles = { [CYCLES_PROJECT_ID]: 5 };
    renderScreen();
    await screen.findByText("Archived alpha");

    await openMenu("Archived alpha");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link"]);
  });
});
