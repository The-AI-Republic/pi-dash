// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-modules tab (MOD-027): skeleton first on a cold entry, then
// the flat archived rows rendered by the shared live row in archived mode.
// The header is the modules header in its archived variant — name search,
// ordering and lead/member/date filters, no status or favorites — and its
// narrowing lives in the archived screen's own browser record. Three
// empties stay distinct, a failed read retries, and rows open the archived
// peek through the address while slice H's shared menu restores from the
// row without leaving the tab.

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
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ArchivedModuleDetail, ArchivedModuleListItem } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import {
  MODULE_RESTORE_SUCCESS_BODY,
  MODULE_RESTORE_SUCCESS_TITLE,
} from "../../modules/components/menus/ModuleRowMenu.js";
import { resetArchivedDisplayCache } from "../../modules/lib/moduleListDisplayStore.js";
import {
  jsonResponse,
  makeDirectoryEntry,
  makeModuleRow,
  makeProjectRow,
  MODULES_LEAD_ID,
  MODULES_PROJECT_ID,
} from "../../modules/testing.js";
import { ArchivedModulesScreen, type ArchivedModulesScreenProps } from "./ArchivedModulesScreen.js";

const FIRST_ID = "323e4567-e89b-12d3-a456-426614174201";
const SECOND_ID = "323e4567-e89b-12d3-a456-426614174202";
const LEAD_ID = "423e4567-e89b-12d3-a456-426614174203";
const ME_ID = "523e4567-e89b-12d3-a456-426614174204";

const LIST_PATH = `/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/archived-modules/`;
const PROJECT_PATH = `/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`;
const TAB_PATH = `/acme/projects/${MODULES_PROJECT_ID}/archives/modules`;

interface Harness {
  rows: ArchivedModuleListItem[];
  listStatus: number;
  holdList: boolean;
  releaseList: (() => void) | null;
  roles: Record<string, number>;
  rolesStatus: number;
  members: ReturnType<typeof makeDirectoryEntry>[];
  restores: number;
  restoreStatus: number;
}

let harness: Harness;
let router: AnyRouter;

function archivedRow(overrides: Partial<ArchivedModuleListItem> = {}): ArchivedModuleListItem {
  return {
    ...makeModuleRow({ status: "completed" }),
    description_text: null,
    description_html: null,
    archived_at: "2026-03-01T00:00:00Z",
    ...overrides,
  } as ArchivedModuleListItem;
}

function archivedDetail(row: ArchivedModuleListItem): ArchivedModuleDetail {
  return {
    ...row,
    link_module: [],
    sub_issues: null,
    total_estimate_points: 0,
    completed_estimate_points: 0,
    backlog_estimate_points: 0,
    unstarted_estimate_points: 0,
    started_estimate_points: 0,
    cancelled_estimate_points: 0,
    estimate_distribution: { assignees: [], labels: [], completion_chart: {} },
    distribution: { assignees: [], labels: [], completion_chart: {} },
  };
}

function seedRows(): ArchivedModuleListItem[] {
  return [
    archivedRow({ id: FIRST_ID, name: "Archived alpha", lead_id: LEAD_ID }),
    archivedRow({ id: SECOND_ID, name: "Archived beta" }),
  ];
}

beforeEach(() => {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  resetArchivedDisplayCache();
  globalThis.localStorage.clear();
  harness = {
    rows: seedRows(),
    listStatus: 200,
    holdList: false,
    releaseList: null,
    roles: { [MODULES_PROJECT_ID]: 20 },
    rolesStatus: 200,
    members: [],
    restores: 0,
    restoreStatus: 204,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (method === "DELETE" && url.includes("/modules/") && url.endsWith("/archive/")) {
      harness.restores += 1;
      if (harness.restoreStatus === 204) {
        harness.rows = harness.rows.filter((row) => !url.includes(`/modules/${row.id}/archive/`));
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
      return jsonResponse(url, harness.listStatus, harness.rows);
    }
    if (method === "GET" && url.includes(LIST_PATH)) {
      const row = harness.rows.find((candidate) => url.includes(`/archived-modules/${candidate.id}/`));
      if (row === undefined) return jsonResponse(url, 404, { detail: "missing" });
      return jsonResponse(url, 200, archivedDetail(row));
    }
    if (method === "GET" && url.endsWith(PROJECT_PATH)) return jsonResponse(url, 200, makeProjectRow());
    if (url.endsWith("/project-roles/")) return jsonResponse(url, harness.rolesStatus, harness.roles);
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, harness.members);
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 200, { id: ME_ID });
    throw new Error(`unexpected request ${method} ${url}`);
  }) as never);
  Object.defineProperty(window, "scrollTo", { value: () => undefined, writable: true });
});

afterEach(() => {
  resetAppClient();
  resetArchivedDisplayCache();
  vi.clearAllMocks();
});

function renderScreen(props: Partial<ArchivedModulesScreenProps> = {}): void {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const tabRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: TAB_PATH,
    component: () => (
      <ArchivedModulesScreen
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        peekModuleId={null}
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
  return screen.getAllByTestId("modules-row").map((row) => row.getAttribute("data-module-name") ?? "");
}

async function openMenu(name: string): Promise<void> {
  // Each row mounts the narrow-screen and the desktop trigger; one opens it.
  const triggers = screen.getAllByRole("button", { name: `Module actions for ${name}` });
  fireEvent.click(triggers[0]!);

  await screen.findAllByRole("menuitem");
}

function menuTitles(): string[] {
  return screen.getAllByRole("menuitem").map((item) => item.textContent?.trim() ?? "");
}

describe("ArchivedModulesScreen", () => {
  it("settles from a cold entry: skeleton first, then the flat archived rows", async () => {
    harness.holdList = true;
    renderScreen();

    expect(await screen.findByTestId("archived-modules-screen")).toBeInTheDocument();
    expect(screen.getByTestId("archived-modules-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-row")).not.toBeInTheDocument();

    await waitFor(() => expect(harness.releaseList).not.toBeNull());
    harness.holdList = false;
    harness.releaseList?.();

    await new Promise((r) => setTimeout(r, 500));
    expect(await screen.findAllByTestId("modules-row")).toHaveLength(2);
    expect(rowNames()).toEqual(["Archived alpha", "Archived beta"]);
    expect(screen.queryByTestId("archived-modules-skeleton")).not.toBeInTheDocument();
    for (const row of screen.getAllByTestId("modules-row")) {
      expect(row).toHaveAttribute("data-module-archived", "true");
    }
  });

  it("renders the zero empty, not a no-match one, for an archive that holds nothing", async () => {
    harness.rows = [];
    renderScreen();

    const empty = await screen.findByTestId("archived-modules-empty");
    expect(empty).toHaveAttribute("data-empty-kind", "zero");
    expect(within(empty).getByRole("heading", { name: "No archived modules yet" })).toBeInTheDocument();
    expect(screen.queryByTestId("modules-row")).not.toBeInTheDocument();
  });

  it("retries a failed read instead of showing an empty archive", async () => {
    harness.listStatus = 500;
    renderScreen();

    expect(await screen.findByTestId("archived-modules-load-error")).toBeInTheDocument();
    expect(screen.queryByTestId("archived-modules-empty")).not.toBeInTheDocument();
    harness.listStatus = 200;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findAllByTestId("modules-row")).not.toHaveLength(0);
  });

  it("narrows through the name search and says the search hid the rows", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-search").querySelector("button") as HTMLElement);
    fireEvent.change(screen.getByTestId("modules-search-input"), { target: { value: "zzz-none" } });

    const empty = await screen.findByTestId("archived-modules-empty");
    expect(empty).toHaveAttribute("data-empty-kind", "search");
    expect(screen.queryByTestId("modules-row")).not.toBeInTheDocument();
    fireEvent.click(within(empty).getByRole("button", { name: "Clear search" }));

    expect(await screen.findAllByTestId("modules-row")).not.toHaveLength(0);
    expect(rowNames()).toHaveLength(2);
  });

  it("narrows by name without hiding the rows that match", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-search").querySelector("button") as HTMLElement);
    fireEvent.change(screen.getByTestId("modules-search-input"), { target: { value: "beta" } });

    await waitFor(() => expect(rowNames()).toEqual(["Archived beta"]));
  });

  it("offers lead, members and both dates — never status or favorites", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const panel = screen.getByTestId("modules-filter-panel");
    const titles = Array.from(panel.querySelectorAll("[data-filter-group]")).map((group) =>
      group.getAttribute("data-filter-group")
    );
    expect(titles).toEqual(["Lead", "Members", "Start date", "Due date"]);
    expect(within(panel).queryByText("Status")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-filter-favorites")).not.toBeInTheDocument();
    // Ordering stays available beside the narrowing.
    expect(screen.getByTestId("modules-sort-trigger")).toBeInTheDocument();
  });

  it("chips an applied filter, reports the filter empty and clears it from there", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const startGroup = Array.from(screen.getAllByTestId("modules-filter-group")).find(
      (group) => group.getAttribute("data-filter-group") === "Start date"
    );
    fireEvent.click(within(startGroup as HTMLElement).getByRole("checkbox", { name: "1 week from now" }));

    const chips = await screen.findByTestId("modules-filter-chips");
    expect(chips).toHaveTextContent(/start date/i);

    const empty = await screen.findByTestId("archived-modules-empty");
    expect(empty).toHaveAttribute("data-empty-kind", "filters");
    expect(screen.queryByTestId("modules-row")).not.toBeInTheDocument();
    fireEvent.click(within(empty).getByRole("button", { name: "Clear filters" }));

    expect(await screen.findAllByTestId("modules-row")).not.toHaveLength(0);
    expect(screen.queryByTestId("modules-filter-chips")).not.toBeInTheDocument();
  });

  it("narrows by lead from the directory once it resolves", async () => {
    harness.rows = [
      archivedRow({ id: FIRST_ID, name: "Archived alpha", lead_id: MODULES_LEAD_ID }),
      archivedRow({ id: SECOND_ID, name: "Archived beta" }),
    ];
    harness.members = [makeDirectoryEntry()];
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const leadGroup = Array.from(screen.getAllByTestId("modules-filter-group")).find(
      (group) => group.getAttribute("data-filter-group") === "Lead"
    );
    fireEvent.click(within(leadGroup as HTMLElement).getByRole("checkbox", { name: "Ada Lovelace" }));

    await waitFor(() => expect(rowNames()).toEqual(["Archived alpha"]));
    expect(screen.getByTestId("modules-filter-chips")).toHaveTextContent(/lead/i);
  });

  it("removes one chip and drops the rest through clear-all", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    const startGroup = Array.from(screen.getAllByTestId("modules-filter-group")).find(
      (group) => group.getAttribute("data-filter-group") === "Start date"
    );
    fireEvent.click(within(startGroup as HTMLElement).getAllByRole("checkbox")[0]!);
    const dueGroup = Array.from(screen.getAllByTestId("modules-filter-group")).find(
      (group) => group.getAttribute("data-filter-group") === "Due date"
    );
    fireEvent.click(within(dueGroup as HTMLElement).getAllByRole("checkbox")[0]!);

    const chips = await screen.findByTestId("modules-filter-chips");
    expect(within(chips).getAllByTestId("modules-filter-chip")).toHaveLength(2);
    fireEvent.click(within(chips).getAllByTestId("modules-filter-chip-clear")[0]!);
    await waitFor(() => expect(within(chips).getAllByTestId("modules-filter-chip")).toHaveLength(1));

    fireEvent.click(within(chips).getByRole("button", { name: "Clear all" }));
    await waitFor(() => expect(screen.queryByTestId("modules-filter-chips")).not.toBeInTheDocument());
  });

  it("opens the archived peek from a row instead of navigating", async () => {
    const onPeekChange = vi.fn();
    renderScreen({ onPeekChange });
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByRole("link", { name: "Archived alpha" }));
    expect(onPeekChange).toHaveBeenCalledWith(FIRST_ID);
    expect(router.state.location.pathname).toBe(TAB_PATH);

    onPeekChange.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Open preview of Archived beta" }));
    expect(onPeekChange).toHaveBeenCalledWith(SECOND_ID);
  });

  it("toggles the open peek shut from the same row", async () => {
    const onPeekChange = vi.fn();
    renderScreen({ onPeekChange, peekModuleId: FIRST_ID });
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByRole("link", { name: "Archived alpha" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("mounts the archived peek from the address, read from the archived record", async () => {
    const onPeekChange = vi.fn();
    renderScreen({ peekModuleId: FIRST_ID, onPeekChange });
    await screen.findAllByTestId("modules-row");

    expect(await screen.findByTestId("modules-peek-body")).toBeInTheDocument();
    expect(screen.getByText("Archived module")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("mounts slice H's menu in archived mode — Restore, never Edit or Delete", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    await openMenu("Archived alpha");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link", "Restore"]);
  });

  it("restores from the row menu, notes where it went and stays on the tab", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    await openMenu("Archived alpha");
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));

    await waitFor(() => expect(harness.restores).toBe(1));
    expect(notify.notifySuccess).toHaveBeenCalledWith(MODULE_RESTORE_SUCCESS_TITLE, MODULE_RESTORE_SUCCESS_BODY);
    await waitFor(() => expect(rowNames()).toEqual(["Archived beta"]));
    expect(router.state.location.pathname).toBe(TAB_PATH);
  });

  it("reports a refused restore and keeps the module archived", async () => {
    harness.restoreStatus = 500;
    renderScreen();
    await screen.findAllByTestId("modules-row");

    await openMenu("Archived alpha");
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));

    await waitFor(() => expect(notify.notifyError).toHaveBeenCalled());
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(rowNames()).toEqual(["Archived alpha", "Archived beta"]);
    expect(router.state.location.pathname).toBe(TAB_PATH);
  });

  it("offers guests the link actions without restore", async () => {
    harness.roles = { [MODULES_PROJECT_ID]: 5 };
    renderScreen();
    await screen.findAllByTestId("modules-row");

    await openMenu("Archived alpha");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link"]);
  });

  it("still lists the archive when the standing cannot be read", async () => {
    harness.rolesStatus = 500;
    renderScreen();

    await screen.findAllByTestId("modules-row");
    expect(rowNames()).toEqual(["Archived alpha", "Archived beta"]);
    await openMenu("Archived alpha");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link"]);
  });

  it("keeps the archived narrowing out of the live list's record", async () => {
    renderScreen();
    await screen.findAllByTestId("modules-row");

    fireEvent.click(screen.getByTestId("modules-search").querySelector("button") as HTMLElement);
    fireEvent.change(screen.getByTestId("modules-search-input"), { target: { value: "alpha" } });
    await waitFor(() => expect(rowNames()).toEqual(["Archived alpha"]));

    const stored = JSON.parse(globalThis.localStorage.getItem("archived_modules_display") ?? "{}") as Record<
      string,
      { query?: string }
    >;
    expect(stored[MODULES_PROJECT_ID]?.query).toBe("alpha");
    expect(globalThis.localStorage.getItem("modules_list_display")).toBeNull();
  });
});
