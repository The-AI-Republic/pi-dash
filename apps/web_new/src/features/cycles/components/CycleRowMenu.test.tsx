// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One cycle row's action menu (CYC-028): the per-state entry matrix
// in order, the archive gate hint, and every entry's wiring — the
// edit dialog, the link helpers, the archive/delete confirmations,
// and the direct restore (CYC-022–026).

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
import userEvent from "@testing-library/user-event";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

const share = vi.hoisted(() => ({ copyCycleLink: vi.fn(), openCycleInNewTab: vi.fn() }));
vi.mock("../lib/cycleShare.js", () => share);

const viewportHarness = vi.hoisted(() => ({ narrow: false }));
vi.mock("../../../shared/shell/index.js", () => ({
  useNarrowViewport: () => viewportHarness.narrow,
}));

vi.mock("../dialog/CycleDialog.js", () => ({
  UpdateCycleDialog: ({
    open,
    onOpenChange,
    cycle,
    currentProject,
  }: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    cycle: { name: string };
    currentProject: { identifier: string };
  }) =>
    open ? (
      <div role="dialog" aria-label="Update cycle">
        <p>
          editing {cycle.name} in {currentProject.identifier}
        </p>
        <button type="button" onClick={() => onOpenChange(false)}>
          Close update
        </button>
      </div>
    ) : null,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  CYCLE_ARCHIVE_LINE,
  CYCLE_ARCHIVE_TITLE,
  CYCLE_DELETED_TITLE,
  CYCLE_PERMISSION_TITLE,
  CYCLE_RESTORED_LINE,
  CYCLE_RESTORED_TITLE,
  CYCLE_RESTORE_FAILED_TITLE,
} from "../api/cycleMutations.js";
import { CYCLE_ARCHIVE_GATE_HINT } from "../lib/cycleMenu.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, makeCycleRow } from "../testing.js";
import { CycleRowMenu } from "./CycleRowMenu.js";

const LIST_PATH = `/acme/projects/${CYCLES_PROJECT_ID}/cycles`;
const DETAIL_PATH = `${LIST_PATH}/${CYCLES_CYCLE_ID}`;

interface Harness {
  deleteStatus: number;
  archiveStatus: number;
  restoreStatus: number;
  deletes: number;
  archives: number;
  restores: number;
}

let harness: Harness;
let router: AnyRouter;

function transport(url: string, init: { method?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  const cycleUrl = `https://api.test/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}/`;
  const archiveUrl = `${cycleUrl}archive/`;
  const respond = (status: number, body: unknown): unknown => ({
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
  if (method === "DELETE" && url === archiveUrl) {
    harness.restores += 1;
    return Promise.resolve(respond(harness.restoreStatus, harness.restoreStatus === 204 ? null : {}));
  }
  if (method === "DELETE" && url === cycleUrl) {
    harness.deletes += 1;
    return Promise.resolve(respond(harness.deleteStatus, harness.deleteStatus === 204 ? null : { error: "forbidden" }));
  }
  if (method === "POST" && url === archiveUrl) {
    harness.archives += 1;
    return Promise.resolve(
      respond(harness.archiveStatus, harness.archiveStatus === 200 ? { archived_at: "2026-03-01T00:00:00Z" } : {})
    );
  }
  throw new Error(`unexpected request ${method} ${url}`);
}

async function renderMenu(
  cycle: CycleRowData,
  canMutate: boolean,
  initialEntry: string = LIST_PATH,
  alwaysVisible?: boolean
): Promise<ReturnType<typeof render>> {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const menuScreen = (): React.ReactElement => (
    <div className="group">
      <CycleRowMenu
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        cycle={cycle}
        canMutate={canMutate}
        currentProject={{ id: CYCLES_PROJECT_ID, name: "Website relaunch", identifier: "WEB" }}
        {...(alwaysVisible === undefined ? {} : { alwaysVisible })}
      />
    </div>
  );
  const listRoute = createRoute({ getParentRoute: () => rootRoute, path: LIST_PATH, component: menuScreen });
  const detailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: DETAIL_PATH,
    component: menuScreen,
  });
  router = createRouter({
    routeTree: rootRoute.addChildren([listRoute, detailRoute]),
    history: createMemoryHistory({ initialEntries: [initialEntry] }),
    defaultPreload: false,
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByRole("button", { name: `Cycle actions for ${cycle.name}` });
  return result;
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

beforeEach(() => {
  harness = { deleteStatus: 204, archiveStatus: 200, restoreStatus: 204, deletes: 0, archives: 0, restores: 0 };
  viewportHarness.narrow = false;
  createAppClient("https://api.test", transport as never);
  Object.defineProperty(window, "scrollTo", { value: () => undefined, writable: true });
});

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("CycleRowMenu", () => {
  it("lists edit, the link actions, the gated archive, and delete on a draft row", async () => {
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    await openMenu("Draft 1");
    expect(menuTitles()).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
    const archive = screen
      .getAllByRole("menuitem")
      .find((item) => within(item).queryByRole("heading", { name: "Archive" }) !== null);
    expect(archive).toHaveAttribute("aria-disabled", "true");
    expect(archive).toHaveTextContent(CYCLE_ARCHIVE_GATE_HINT);
    expect(screen.getByRole("separator")).toBeInTheDocument();
  });

  it("drops edit and delete and enables archive on a finished row", async () => {
    await renderMenu(makeCycleRow({ name: "Done 1", status: "COMPLETED" }), true);
    await openMenu("Done 1");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link", "Archive"]);
    const archive = screen
      .getAllByRole("menuitem")
      .find((item) => within(item).queryByRole("heading", { name: "Archive" }) !== null);
    expect(archive).not.toHaveAttribute("aria-disabled", "true");
    expect(archive).not.toHaveTextContent(CYCLE_ARCHIVE_GATE_HINT);
  });

  it("swaps archive for restore on an archived row", async () => {
    await renderMenu(makeCycleRow({ name: "Old 1", status: "COMPLETED", archived_at: "2026-03-01T00:00:00Z" }), true);
    await openMenu("Old 1");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link", "Restore"]);
  });

  it("shows guests only the link actions with no separator", async () => {
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), false);
    await openMenu("Draft 1");
    expect(menuTitles()).toEqual(["Open in new tab", "Copy link"]);
    expect(screen.queryByRole("separator")).not.toBeInTheDocument();
  });

  it("opens the edit dialog on the row's record and closes it", async () => {
    const user = userEvent.setup();
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    await openMenu("Draft 1");
    pickEntry("Edit");
    expect(await screen.findByRole("dialog", { name: "Update cycle" })).toHaveTextContent("editing Draft 1 in WEB");
    await user.click(screen.getByRole("button", { name: "Close update" }));
    expect(screen.queryByRole("dialog", { name: "Update cycle" })).not.toBeInTheDocument();
  });

  it("copies the link and opens a new tab through the shared helpers", async () => {
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    await openMenu("Draft 1");
    pickEntry("Copy link");
    expect(share.copyCycleLink).toHaveBeenCalledWith("acme", CYCLES_PROJECT_ID, CYCLES_CYCLE_ID);
    await openMenu("Draft 1");
    pickEntry("Open in new tab");
    expect(share.openCycleInNewTab).toHaveBeenCalledWith("acme", CYCLES_PROJECT_ID, CYCLES_CYCLE_ID);
  });

  it("deletes through confirmation, confirms, and returns to the list", async () => {
    const user = userEvent.setup();
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true, DETAIL_PATH);
    await openMenu("Draft 1");
    pickEntry("Delete");
    expect(await screen.findByRole("heading", { name: "Delete cycle" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(harness.deletes).toBe(1);
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_DELETED_TITLE);
    expect(screen.queryByRole("heading", { name: "Delete cycle" })).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("cancels the delete without a write", async () => {
    const user = userEvent.setup();
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    await openMenu("Draft 1");
    pickEntry("Delete");
    await screen.findByRole("heading", { name: "Delete cycle" });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(harness.deletes).toBe(0);
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Delete cycle" })).not.toBeInTheDocument();
  });

  it("keeps the delete dialog open when the write is refused", async () => {
    const user = userEvent.setup();
    harness.deleteStatus = 403;
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    await openMenu("Draft 1");
    pickEntry("Delete");
    await screen.findByRole("heading", { name: "Delete cycle" });
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_PERMISSION_TITLE);
    expect(screen.getByRole("heading", { name: "Delete cycle" })).toBeInTheDocument();
  });

  it("archives a finished cycle through confirmation", async () => {
    const user = userEvent.setup();
    await renderMenu(makeCycleRow({ name: "Done 1", status: "COMPLETED" }), true);
    await openMenu("Done 1");
    pickEntry("Archive");
    expect(await screen.findByRole("heading", { name: "Archive cycle Done 1" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Archive" }));
    expect(harness.archives).toBe(1);
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_ARCHIVE_TITLE, CYCLE_ARCHIVE_LINE);
    expect(screen.queryByRole("heading", { name: "Archive cycle Done 1" })).not.toBeInTheDocument();
  });

  it("cancels the archive without a write", async () => {
    const user = userEvent.setup();
    await renderMenu(makeCycleRow({ name: "Done 1", status: "COMPLETED" }), true);
    await openMenu("Done 1");
    pickEntry("Archive");
    await screen.findByRole("heading", { name: "Archive cycle Done 1" });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(harness.archives).toBe(0);
    expect(screen.queryByRole("heading", { name: "Archive cycle Done 1" })).not.toBeInTheDocument();
  });

  it("restores straight from the menu with no dialog", async () => {
    await renderMenu(makeCycleRow({ name: "Old 1", status: "COMPLETED", archived_at: "2026-03-01T00:00:00Z" }), true);
    await openMenu("Old 1");
    pickEntry("Restore");
    await vi.waitFor(() => expect(harness.restores).toBe(1));
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_RESTORED_TITLE, CYCLE_RESTORED_LINE);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("reports a failed restore and leaves the row alone", async () => {
    harness.restoreStatus = 404;
    await renderMenu(makeCycleRow({ name: "Old 1", status: "COMPLETED", archived_at: "2026-03-01T00:00:00Z" }), true);
    await openMenu("Old 1");
    pickEntry("Restore");
    await vi.waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_RESTORE_FAILED_TITLE, expect.any(String))
    );
    expect(notify.notifySuccess).not.toHaveBeenCalled();
  });

  it("mounts the hover trigger on wide viewports with no inline twin", async () => {
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    expect(screen.getByTestId("cycle-row-menu-trigger")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-row-menu-inline")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Cycle actions for Draft 1" })).toHaveLength(1);
  });

  it("reveals the trigger on hover by default and always for header mounts", async () => {
    const first = await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    expect(screen.getByTestId("cycle-row-menu-trigger").className).toContain("opacity-0");
    first.unmount();
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true, LIST_PATH, true);
    expect(screen.getByTestId("cycle-row-menu-trigger").className).not.toContain("opacity-0");
  });

  it("mounts the menu trigger inline on narrow viewports with the same entries", async () => {
    viewportHarness.narrow = true;
    await renderMenu(makeCycleRow({ name: "Draft 1", status: "draft" }), true);
    expect(screen.getByTestId("cycle-row-menu-inline")).toBeInTheDocument();
    expect(screen.queryByTestId("cycle-row-menu-trigger")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Cycle actions for Draft 1" })).toHaveLength(1);
    await openMenu("Draft 1");
    expect(menuTitles()).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
  });
});
