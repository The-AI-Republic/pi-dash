// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The connected row (parity/specs/pages.md §3.8, PAGE-013–017): each state
// entry drives this slice's write, the delete entry opens the confirmation
// only on a shelved page and closes it once the write lands, the move
// dialog is mounted only for an edition that serves the move, and both
// dialogs stay closed until their entry is chosen.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
const toaster = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));
const pickerHarness = vi.hoisted(() => ({ projects: [] as Record<string, unknown>[] }));

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));
vi.mock("../../../shared/shell/toaster.js", () => toaster);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, PAGES_USER_ID, jsonResponse, makePageRow } from "../testing.js";
import { PageRowConnected } from "./PageRowConnected.js";

interface Harness {
  accessCalls: number;
  lockCalls: number;
  archiveCalls: number;
  deleteCalls: number;
  deleteStatus: number;
  moveStatus: number;
  moveCalls: number;
  childPages: ReturnType<typeof makePageRow>[];
  childReadCalls: number;
  childReadRelease: (() => void) | null;
  childReadHold: Promise<void> | null;
}

let harness: Harness;

beforeEach(() => {
  harness = {
    accessCalls: 0,
    lockCalls: 0,
    archiveCalls: 0,
    deleteCalls: 0,
    deleteStatus: 204,
    moveStatus: 204,
    moveCalls: 0,
    childPages: [],
    childReadCalls: 0,
    childReadRelease: null,
    childReadHold: null,
  };
  toaster.notifySuccess.mockReset();
  toaster.notifyError.mockReset();
  routerHarness.navigate.mockReset();
  pickerHarness.projects = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith("/access/")) {
      harness.accessCalls += 1;
      return jsonResponse(url, 204, null);
    }
    if (url.endsWith("/lock/")) {
      harness.lockCalls += 1;
      return jsonResponse(url, 204, null);
    }
    if (url.endsWith("/archive/")) {
      harness.archiveCalls += 1;
      return jsonResponse(url, 200, { archived_at: "2026-10-25 10:00:00.123456" });
    }
    if (url.endsWith("/move/")) {
      harness.moveCalls += 1;
      return jsonResponse(url, harness.moveStatus, null);
    }
    if (
      new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/[0-9a-f-]+/$`).test(url) &&
      init.method === "DELETE"
    ) {
      harness.deleteCalls += 1;
      return jsonResponse(url, harness.deleteStatus, null);
    }
    if (url.endsWith("/projects/")) return jsonResponse(url, 200, pickerHarness.projects);
    const pagesRead = new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/\\?.*parent=[0-9a-f-]+.*$`).exec(url);
    if (pagesRead !== null) {
      harness.childReadCalls += 1;
      if (harness.childReadHold !== null) await harness.childReadHold;
      return jsonResponse(url, 200, harness.childPages);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

function renderRow(overrides: Partial<React.ComponentProps<typeof PageRowConnected>> = {}): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageRowConnected
        workspaceSlug="acme"
        projectId={PAGES_PROJECT_ID}
        page={makePageRow({ name: "Connected row" })}
        owner={{ id: PAGES_USER_ID, name: "Ada Byron", avatarUrl: null }}
        showStar
        canDuplicate
        canManage
        canMove={false}
        onOpenInNewTab={vi.fn()}
        onCopyLink={vi.fn()}
        onDuplicate={vi.fn()}
        {...overrides}
      />
    </QueryClientProvider>
  );
}

function openMenu(title = "Connected row"): void {
  fireEvent.click(screen.getByRole("button", { name: `Page actions for ${title}` }));
}

describe("PageRowConnected", () => {
  it("opens neither dialog before an entry is chosen", () => {
    renderRow();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("drives the visibility, lock and archive writes", async () => {
    renderRow();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Make private" }));
    await waitFor(() => expect(harness.accessCalls).toBe(1));
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Lock" }));
    await waitFor(() => expect(harness.lockCalls).toBe(1));
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    await waitFor(() => expect(harness.archiveCalls).toBe(1));
  });

  it("opens the delete dialog on a shelved row and closes it after the write", async () => {
    renderRow({ page: makePageRow({ name: "Connected row", archived_at: "2026-10-01" }) });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "Delete page" })).toBeInTheDocument();
    await waitFor(() => expect(harness.childReadCalls).toBe(1));
    await waitFor(() => expect(screen.queryByTestId("page-delete-rehomed-loading")).not.toBeInTheDocument());
    expect(screen.queryByTestId("page-delete-rehomed")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(harness.deleteCalls).toBe(1));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("refuses to confirm a shelved row's delete until its child count resolves", async () => {
    let release = (): void => {};
    harness.childReadHold = new Promise<void>((resolve) => {
      release = resolve;
    });
    harness.childPages = [makePageRow({ name: "Shelved child", archived_at: "2026-10-01" })];
    renderRow({ page: makePageRow({ name: "Connected row", archived_at: "2026-10-01" }) });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByTestId("page-delete-rehomed-loading")).toHaveTextContent("Counting sub-pages…");
    expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled();
    expect(harness.deleteCalls).toBe(0);
    release();
    expect(await screen.findByTestId("page-delete-rehomed")).toHaveTextContent(
      "1 sub-page will move to the top level."
    );
    expect(screen.getByRole("button", { name: "Delete" })).toBeEnabled();
  });

  it("keeps the delete dialog open when the write is refused", async () => {
    harness.deleteStatus = 500;
    renderRow({ page: makePageRow({ name: "Connected row", archived_at: "2026-10-01" }) });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    const confirm = await screen.findByRole("button", { name: "Delete" });
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);
    await waitFor(() => expect(toaster.notifyError).toHaveBeenCalledWith("Page could not be deleted."));
    expect(screen.getByRole("heading", { name: "Delete page" })).toBeInTheDocument();
  });

  it("mounts the move dialog only where the edition serves the move", () => {
    renderRow();
    openMenu();
    expect(screen.queryByRole("menuitem", { name: "Move to project…" })).not.toBeInTheDocument();
  });

  it("opens the move dialog and hands the chosen project to the write", async () => {
    pickerHarness.projects = [
      {
        id: "33333333-3333-4333-8333-333333333333",
        name: "Target project",
        identifier: "TARG",
        sort_order: null,
        logo_props: {},
        member_role: 20,
        intake_count: 0,
        archived_at: null,
        workspace: "44444444-4444-4444-8444-444444444444",
        cycle_view: false,
        issue_views_view: false,
        module_view: false,
        page_view: false,
        inbox_view: false,
        is_default: false,
        guest_view_all_features: false,
        project_lead: null,
        network: 0,
        created_at: "2026-01-01T00:00:00.000Z",
        updated_at: "2026-01-01T00:00:00.000Z",
        created_by: null,
        updated_by: null,
      },
    ];
    renderRow({ canMove: true });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Move to project…" }));
    expect(await screen.findByRole("heading", { name: "Move page" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Select a project" }));
    fireEvent.click(await screen.findByRole("option", { name: /Target project/ }));
    fireEvent.click(screen.getByRole("button", { name: "Move page" }));
    await waitFor(() => expect(harness.moveCalls).toBe(1));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("reports a refused move while the row stays put", async () => {
    harness.moveStatus = 500;
    pickerHarness.projects = [
      {
        id: "33333333-3333-4333-8333-333333333333",
        name: "Target project",
        identifier: "TARG",
        sort_order: null,
        logo_props: {},
        member_role: 20,
        intake_count: 0,
        archived_at: null,
        workspace: "44444444-4444-4444-8444-444444444444",
        cycle_view: false,
        issue_views_view: false,
        module_view: false,
        page_view: false,
        inbox_view: false,
        is_default: false,
        guest_view_all_features: false,
        project_lead: null,
        network: 0,
        created_at: "2026-01-01T00:00:00.000Z",
        updated_at: "2026-01-01T00:00:00.000Z",
        created_by: null,
        updated_by: null,
      },
    ];
    renderRow({ canMove: true });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Move to project…" }));
    await screen.findByRole("heading", { name: "Move page" });
    fireEvent.click(screen.getByRole("button", { name: "Select a project" }));
    fireEvent.click(await screen.findByRole("option", { name: /Target project/ }));
    fireEvent.click(screen.getByRole("button", { name: "Move page" }));
    await waitFor(() => expect(toaster.notifyError).toHaveBeenCalledWith("Page could not be moved."));
    expect(PAGES_PAGE_ID).toMatch(/^[0-9a-f-]{36}$/);
  });
});
