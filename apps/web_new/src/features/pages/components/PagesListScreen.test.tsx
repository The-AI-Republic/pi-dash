// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-001–006 screen integration: gate, one collection read, tabs,
// search, sort, filters, empty/error/no-results and row links.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PageRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
const toaster = vi.hoisted(() => ({ notifyPending: vi.fn(), notifySuccess: vi.fn(), notifyError: vi.fn() }));
const favoritesShell = vi.hoisted(() => ({ isFavoritesSectionOpen: vi.fn(), setFavoritesSectionOpen: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));
vi.mock("../../../shared/shell/toaster.js", () => toaster);
vi.mock("../../../shared/shell/favorites.js", () => favoritesShell);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  PAGES_PROJECT_ID,
  PAGES_USER_ID,
  jsonResponse,
  makePageRow,
  makeProjectDetails,
  makeProjectRow,
  makeWorkspaceMember,
} from "../testing.js";
import { DEFAULT_PAGE_SORT } from "../lib/pageList.js";
import { pageKeys } from "../api/keys.js";
import { clearCommands, findCommand } from "../../../shared/commands/index.js";
import { CREATE_PAGE_COMMAND_ID } from "./PagesCreateEntry.js";
import { PagesListScreen } from "./PagesListScreen.js";

interface Harness {
  createdId: string;
  createStatus: number;
  createCalls: number;
  duplicateStatus: number;
  duplicateCalls: number;
  favoriteStatus: number;
  favoriteCalls: number;
  accessStatus: number;
  accessCalls: number;
  lockStatus: number;
  lockCalls: number;
  archiveStatus: number;
  archiveCalls: number;
  deleteStatus: number;
  deleteCalls: number;
  moveStatus: number;
  moveCalls: number;
  project: Record<string, unknown>;
  projectStatus: number;
  roles: Record<string, number>;
  pages: PageRow[];
  pagesStatus: number;
  pagesCalls: number;
  pagesHold: Promise<void> | null;
  members: Record<string, unknown>[];
  meStatus: number;
  meCalls: number;
}

let harness: Harness;

/** Stable valid row ids: the collection contract validates UUIDs. */
const pageId = (suffix: number): string => `323e4567-e89b-12d3-a456-4266141741${String(suffix).padStart(2, "0")}`;

beforeEach(() => {
  harness = {
    createdId: pageId(9),
    createStatus: 201,
    createCalls: 0,
    duplicateStatus: 200,
    duplicateCalls: 0,
    favoriteStatus: 204,
    favoriteCalls: 0,
    accessStatus: 204,
    accessCalls: 0,
    lockStatus: 204,
    lockCalls: 0,
    archiveStatus: 200,
    archiveCalls: 0,
    deleteStatus: 204,
    deleteCalls: 0,
    moveStatus: 204,
    moveCalls: 0,
    project: makeProjectRow(),
    projectStatus: 200,
    roles: { [PAGES_PROJECT_ID]: 20 },
    pages: [],
    pagesStatus: 200,
    pagesCalls: 0,
    pagesHold: null,
    members: [makeWorkspaceMember(PAGES_USER_ID, "Ada Byron")],
    meStatus: 200,
    meCalls: 0,
  };
  routerHarness.navigate.mockReset();
  toaster.notifyPending.mockReset();
  toaster.notifyPending.mockReturnValue({ settle: vi.fn() });
  toaster.notifySuccess.mockReset();
  toaster.notifyError.mockReset();
  favoritesShell.isFavoritesSectionOpen.mockReset();
  favoritesShell.isFavoritesSectionOpen.mockReturnValue(false);
  favoritesShell.setFavoritesSectionOpen.mockReset();
  createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    const projectRead = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/$`).exec(url);
    if (projectRead !== null) {
      const id = projectRead[1] as string;
      return jsonResponse(url, harness.projectStatus, { ...harness.project, id });
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith("/projects/details/")) return jsonResponse(url, 200, [makeProjectDetails()]);
    if (url.endsWith("/api/users/me/")) {
      harness.meCalls += 1;
      if (harness.meStatus === -1) return new Promise(() => undefined) as never;
      return jsonResponse(url, harness.meStatus, {
        id: PAGES_USER_ID,
        email: "ada@test.example",
        username: "ada",
        first_name: "Ada",
        last_name: "Byron",
        display_name: "Ada Byron",
        avatar: "",
        avatar_url: null,
        cover_image: null,
        cover_image_url: null,
        date_joined: "2026-01-01T00:00:00.000Z",
        user_timezone: "UTC",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        is_password_autoset: false,
        last_login_medium: "password",
        last_login_time: null,
      });
    }
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, harness.members);
    const memberRead = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/members/$`).exec(url);
    if (memberRead !== null) {
      const projectId = memberRead[1] as string;
      return jsonResponse(url, 200, [
        {
          id: "523e4567-e89b-12d3-a456-426614174004",
          role: 20,
          member: PAGES_USER_ID,
          project: projectId,
          original_role: 20,
          created_at: "2026-01-02T03:04:05Z",
        },
      ]);
    }
    const accessWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/([0-9a-f-]+)/access/$`).exec(url);
    if (accessWrite !== null) {
      harness.accessCalls += 1;
      return jsonResponse(url, harness.accessStatus, null);
    }
    const lockWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/([0-9a-f-]+)/lock/$`).exec(url);
    if (lockWrite !== null) {
      harness.lockCalls += 1;
      return jsonResponse(url, harness.lockStatus, null);
    }
    const archiveWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/([0-9a-f-]+)/archive/$`).exec(
      url
    );
    if (archiveWrite !== null) {
      harness.archiveCalls += 1;
      return jsonResponse(url, harness.archiveStatus, { archived_at: "2026-10-25 10:00:00.123456" });
    }
    const deleteWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/([0-9a-f-]+)/$`).exec(url);
    if (deleteWrite !== null && init.method === "DELETE") {
      harness.deleteCalls += 1;
      return jsonResponse(url, harness.deleteStatus, null);
    }
    const moveWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/([0-9a-f-]+)/move/$`).exec(url);
    if (moveWrite !== null) {
      harness.moveCalls += 1;
      return jsonResponse(url, harness.moveStatus, null);
    }
    const favoriteWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/favorite-pages/([0-9a-f-]+)/$`).exec(
      url
    );
    if (favoriteWrite !== null) {
      expect(init.method).toMatch(/^(POST|DELETE)$/);
      harness.favoriteCalls += 1;
      return jsonResponse(url, harness.favoriteStatus, null);
    }
    const duplicateWrite = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/([0-9a-f-]+)/duplicate/$`).exec(
      url
    );
    if (duplicateWrite !== null) {
      harness.duplicateCalls += 1;
      return jsonResponse(url, harness.duplicateStatus, {
        ...makePageRow({ name: "Launch notes (Copy)", parent: null }),
        description_html: "",
      });
    }
    const pagesUrl = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/$`).exec(url);
    if (pagesUrl !== null) {
      if (init.method !== "GET") {
        harness.createCalls += 1;
        return jsonResponse(url, harness.createStatus, {
          ...makePageRow({ id: harness.createdId, name: "Untitled" }),
          description_html: "",
        });
      }
      harness.pagesCalls += 1;
      if (harness.pagesHold !== null) await harness.pagesHold;
      return jsonResponse(url, harness.pagesStatus, harness.pages);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  clearCommands();
  document.title = "";
});

function renderScreen(tab: "public" | "private" | "archived" = "public"): ReturnType<typeof render> {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PagesListScreen workspaceSlug="acme" projectId={PAGES_PROJECT_ID} tab={tab} />
    </QueryClientProvider>
  );
}

function renderProject(projectId: string, client: QueryClient): ReturnType<typeof render> {
  return render(
    <QueryClientProvider client={client}>
      <PagesListScreen workspaceSlug="acme" projectId={projectId} tab="public" />
    </QueryClientProvider>
  );
}

describe("PagesListScreen", () => {
  it("gates the list and sends admins to the pages feature leaf", async () => {
    harness.project = makeProjectRow({ page_view: false });
    renderScreen();
    expect(await screen.findByTestId("pages-feature-gate")).toBeInTheDocument();
    expect(screen.queryByTestId("pages-list-tabs")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Manage features" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/settings/projects/${PAGES_PROJECT_ID}/features/pages`,
    });
    expect(harness.pagesCalls).toBe(0);
  });

  it("disables the gate shortcut for a non-admin", async () => {
    harness.project = makeProjectRow({ page_view: false });
    harness.roles = { [PAGES_PROJECT_ID]: 15 };
    renderScreen();
    expect(await screen.findByTestId("pages-feature-gate")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled();
  });

  it("shows the zero state without a tab strip and hides the guest create action", async () => {
    harness.roles = { [PAGES_PROJECT_ID]: 5 };
    renderScreen();
    expect(await screen.findByTestId("pages-empty-state")).toBeInTheDocument();
    expect(screen.queryByTestId("pages-list-tabs")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create your first Page" })).toBeDisabled();
  });

  it("shows the archived empty state without a create action", async () => {
    renderScreen("archived");
    expect(await screen.findByTestId("pages-empty-state")).toBeInTheDocument();
    expect(screen.getByText("No archived pages yet")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create your first Page" })).not.toBeInTheDocument();
  });

  it("falls through to the ordinary empty state when the archived read fails", async () => {
    harness.pagesStatus = 500;
    renderScreen("archived");
    expect(await screen.findByTestId("pages-empty-state")).toBeInTheDocument();
    expect(screen.getByText("No pages in this project yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create your first Page" })).toBeEnabled();
  });

  it("keeps the guest create action disabled after a failed read", async () => {
    harness.roles = { [PAGES_PROJECT_ID]: 5 };
    harness.pagesStatus = 500;
    renderScreen();
    expect(await screen.findByTestId("pages-empty-state")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create your first Page" })).toBeDisabled();
  });

  it("renders the empty fallback when the project read fails instead of a blank panel", async () => {
    harness.projectStatus = 500;
    renderScreen();
    expect(await screen.findByTestId("pages-empty-state")).toBeInTheDocument();
    expect(screen.getByText("No pages in this project yet")).toBeInTheDocument();
    expect(screen.getByTestId("pages-list-screen")).not.toBeEmptyDOMElement();
    expect(harness.pagesCalls).toBe(0);
  });

  it("keeps cached rows and hides the skeleton when a background refetch fails", async () => {
    harness.pagesStatus = 500;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(pageKeys.list("acme", PAGES_PROJECT_ID), [makePageRow({ name: "Cached page" })]);
    renderProject(PAGES_PROJECT_ID, client);
    expect(await screen.findByRole("link", { name: "Cached page" })).toBeInTheDocument();

    await act(async () => {
      await client.refetchQueries({ queryKey: pageKeys.list("acme", PAGES_PROJECT_ID) });
    });

    expect(screen.getByRole("link", { name: "Cached page" })).toBeInTheDocument();
    expect(screen.queryByTestId("pages-list-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("pages-empty-state")).not.toBeInTheDocument();
    expect(harness.pagesCalls).toBe(1);
  });

  it("shows a lightweight refresh indication while a refetch runs over cached rows and clears it after", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    harness.pages = [makePageRow({ name: "Cached page" })];
    client.setQueryData(pageKeys.list("acme", PAGES_PROJECT_ID), harness.pages);
    let release = (): void => undefined;
    harness.pagesHold = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderProject(PAGES_PROJECT_ID, client);
    expect(await screen.findByRole("link", { name: "Cached page" })).toBeInTheDocument();
    expect(screen.queryByTestId("pages-refresh-status")).not.toBeInTheDocument();

    let refetch: Promise<unknown> = Promise.resolve();
    await act(async () => {
      refetch = client.refetchQueries({ queryKey: pageKeys.list("acme", PAGES_PROJECT_ID) });
      await Promise.resolve();
    });

    expect(await screen.findByTestId("pages-refresh-status")).toBeInTheDocument();
    expect(screen.getByTestId("pages-refresh-status")).toHaveTextContent("Refreshing pages");
    expect(screen.getByRole("link", { name: "Cached page" })).toBeInTheDocument();
    expect(screen.queryByTestId("pages-list-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("pages-empty-state")).not.toBeInTheDocument();

    await act(async () => {
      release();
      await refetch;
    });

    await waitFor(() => expect(screen.queryByTestId("pages-refresh-status")).not.toBeInTheDocument());
    expect(screen.getByRole("link", { name: "Cached page" })).toBeInTheDocument();
  });

  it("renders rows in modified-descending order once loaded", async () => {
    harness.pages = [
      makePageRow({ id: pageId(1), name: "Older", updated_at: "2026-01-01T00:00:00Z" }),
      makePageRow({ id: pageId(2), name: "Newer", updated_at: "2026-02-01T00:00:00Z" }),
    ];
    renderScreen();
    await screen.findByTestId("pages-list");
    expect(screen.getAllByTestId("page-row").map((row) => row.textContent)).toEqual(
      expect.arrayContaining([expect.stringContaining("Newer"), expect.stringContaining("Older")])
    );
    expect(screen.getByRole("button", { name: "Date modified" })).toBeInTheDocument();
    expect(DEFAULT_PAGE_SORT).toEqual({ key: "modified", direction: "descending" });
  });

  it("switches tabs client-side without refetching", async () => {
    harness.pages = [
      makePageRow({ id: pageId(3), name: "Open page", access: 0 }),
      makePageRow({ id: pageId(4), name: "Private page", access: 1 }),
    ];
    renderScreen();
    await screen.findByTestId("pages-list");
    expect(screen.getByRole("link", { name: "Open page" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Private page" })).not.toBeInTheDocument();
    expect(harness.pagesCalls).toBe(1);
  });

  it("narrows with search and shows no-results on zero matches", async () => {
    harness.pages = [makePageRow({ name: "Alpha" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.change(screen.getByPlaceholderText("Search pages"), { target: { value: "zzz" } });
    expect(await screen.findByTestId("pages-no-results")).toBeInTheDocument();
    expect(screen.queryByTestId("page-row")).not.toBeInTheDocument();
    expect(harness.pagesCalls).toBe(1);
  });

  it("applies a favorites filter and restores rows by clearing it", async () => {
    harness.pages = [
      makePageRow({ id: pageId(5), name: "Starred", is_favorite: true }),
      makePageRow({ id: pageId(6), name: "Plain", is_favorite: false }),
    ];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    fireEvent.click(screen.getByRole("button", { name: "Favorites" }));
    expect(screen.getByRole("link", { name: "Starred" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Plain" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove Favorites" }));
    expect(screen.getByRole("link", { name: "Plain" })).toBeInTheDocument();
  });

  it("sorts by name and direction immediately", async () => {
    harness.pages = [
      makePageRow({ id: pageId(7), name: "Bravo", updated_at: "2026-03-01T00:00:00Z" }),
      makePageRow({ id: pageId(8), name: "Alpha", updated_at: "2026-02-01T00:00:00Z" }),
    ];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Name" }));
    fireEvent.click(screen.getByRole("button", { name: "Name" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Ascending" }));
    await waitFor(() => {
      expect(screen.getAllByTestId("page-row").map((row) => row.getAttribute("data-page-id"))).toEqual([
        pageId(8),
        pageId(7),
      ]);
    });
    expect(harness.pagesCalls).toBe(1);
  });

  it("falls through to the create-enabled empty state on a failed collection read", async () => {
    harness.pagesStatus = 500;
    renderScreen();
    expect(await screen.findByTestId("pages-empty-state")).toBeInTheDocument();
    expect(screen.getByText("No pages in this project yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create your first Page" })).toBeEnabled();
  });

  it("opens and copies a row link", async () => {
    harness.pages = [makePageRow({ name: "Share me" })];
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Share me" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in new tab" }));
    expect(open).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Share me" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalled());
    open.mockRestore();
  });

  it("falls back to an unknown owner when the directory lacks the member", async () => {
    harness.pages = [makePageRow({ name: "Orphan", owned_by: "623e4567-e89b-12d3-a456-426614174005" })];
    renderScreen();
    expect(await screen.findByRole("img", { name: "Unknown member" })).toBeInTheDocument();
  });

  it("keeps the list working when the session read fails", async () => {
    harness.meStatus = 500;
    harness.pages = [makePageRow({ name: "No session", owned_by: PAGES_USER_ID })];
    renderScreen();
    expect(await screen.findByRole("link", { name: "No session" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    expect(screen.getByRole("button", { name: /Ada Byron/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /You/ })).not.toBeInTheDocument();
  });

  it("does not label a creator as You before the session resolves", async () => {
    harness.meStatus = -1;
    harness.pages = [makePageRow({ name: "Pending session", owned_by: PAGES_USER_ID })];
    renderScreen();
    expect(await screen.findByRole("link", { name: "Pending session" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    expect(screen.getByRole("button", { name: /Ada Byron/ })).toBeInTheDocument();
  });

  it("resets client discovery state when the project changes", async () => {
    harness.pages = [makePageRow({ name: "Alpha" })];
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const otherProjectId = "723e4567-e89b-12d3-a456-426614174006";
    const view = renderProject(otherProjectId, client);
    await screen.findByRole("link", { name: "Alpha" });
    view.rerender(
      <QueryClientProvider client={client}>
        <PagesListScreen workspaceSlug="acme" projectId={PAGES_PROJECT_ID} tab="public" />
      </QueryClientProvider>
    );
    await screen.findByRole("link", { name: "Alpha" });
    fireEvent.click(screen.getByRole("button", { name: "Open page search" }));
    fireEvent.change(screen.getByPlaceholderText("Search pages"), { target: { value: "zzz" } });
    expect(await screen.findByTestId("pages-no-results")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "ada" } });
    fireEvent.change(screen.getByLabelText("Custom creation date"), { target: { value: "2026-10-20" } });
    expect(screen.getByPlaceholderText("Search")).toHaveValue("ada");
    view.rerender(
      <QueryClientProvider client={client}>
        <PagesListScreen workspaceSlug="acme" projectId={otherProjectId} tab="public" />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("link", { name: "Alpha" })).toBeInTheDocument();
    expect(screen.queryByPlaceholderText("Search pages")).not.toBeInTheDocument();
    expect(screen.queryByTestId("pages-filter-panel")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Filters" }));
    expect(screen.getByPlaceholderText("Search")).toHaveValue("");
    expect(screen.getByLabelText("Custom creation date")).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Date modified" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Name" }));
    expect(screen.getByRole("button", { name: "Name" })).toBeInTheDocument();
  });
});

describe("PagesListScreen creation and row writes (PAGE-007–012)", () => {
  it("quick-creates an untitled page from the header and lands on its detail", async () => {
    renderScreen();
    const quick = await screen.findByTestId("pages-quick-create");
    expect(quick).toHaveTextContent("Add page");
    fireEvent.click(quick);
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({
        href: `/acme/projects/${PAGES_PROJECT_ID}/pages/${pageId(9)}`,
      })
    );
    expect(harness.createCalls).toBe(1);
  });

  it("creates from the dialog through the palette command and lands on the new page", async () => {
    renderScreen();
    await screen.findByTestId("pages-quick-create");
    await act(async () => {
      findCommand(CREATE_PAGE_COMMAND_ID)?.run();
    });
    expect(await screen.findByRole("dialog", { name: "Create page" })).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Title"), { target: { value: "From dialog" } });
    fireEvent.click(screen.getByRole("button", { name: "Create Page" }));
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({
        href: `/acme/projects/${PAGES_PROJECT_ID}/pages/${pageId(9)}`,
      })
    );
    expect(toaster.notifyError).not.toHaveBeenCalled();
  });

  it("hides every create control from a guest", async () => {
    harness.roles = { [PAGES_PROJECT_ID]: 5 };
    harness.pages = [makePageRow({ name: "Guest row" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    expect(screen.queryByTestId("pages-quick-create")).not.toBeInTheDocument();
    expect(findCommand(CREATE_PAGE_COMMAND_ID)).toBeUndefined();
    expect(screen.queryByTestId("page-row-star")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Guest row" }));
    expect(screen.queryByRole("menuitem", { name: "Make a copy" })).not.toBeInTheDocument();
  });

  it("keeps the list and reports when quick create fails", async () => {
    harness.createStatus = 500;
    harness.pages = [makePageRow({ name: "Existing" })];
    renderScreen();
    fireEvent.click(await screen.findByTestId("pages-quick-create"));
    await waitFor(() => expect(toaster.notifyError).toHaveBeenCalledWith("Page could not be created."));
    expect(routerHarness.navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("link", { name: "Existing" })).toBeInTheDocument();
  });

  it("creates a page from the empty state's action", async () => {
    renderScreen();
    fireEvent.click(await screen.findByRole("button", { name: "Create your first Page" }));
    await waitFor(() => expect(harness.createCalls).toBe(1));
    await waitFor(() =>
      expect(routerHarness.navigate).toHaveBeenCalledWith({
        href: `/acme/projects/${PAGES_PROJECT_ID}/pages/${pageId(9)}`,
      })
    );
  });

  it("stars a row through its own favourite route and opens the favourites section", async () => {
    harness.pages = [makePageRow({ name: "Star me" })];
    renderScreen();
    const star = await screen.findByRole("button", { name: "Add Star me to favorites" });
    fireEvent.click(star);
    await waitFor(() => expect(harness.favoriteCalls).toBe(1));
    await waitFor(() => expect(favoritesShell.setFavoritesSectionOpen).toHaveBeenCalledWith(true));
  });

  it("duplicates a row and re-reads the collection so the copy appears", async () => {
    harness.pages = [makePageRow({ name: "Launch notes" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Launch notes" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Make a copy" }));
    await waitFor(() => expect(harness.duplicateCalls).toBe(1));
    expect(toaster.notifySuccess).toHaveBeenCalledWith("Page duplicated successfully.");
  });

  it("offers no duplicate entry to a guest", async () => {
    harness.roles = { [PAGES_PROJECT_ID]: 5 };
    harness.pages = [makePageRow({ name: "Read only", owned_by: "623e4567-e89b-12d3-a456-426614174005" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Read only" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Open in new tab", "Copy link"]);
  });

  it("gives the page owner the state switches and hides them from a member", async () => {
    harness.roles = { [PAGES_PROJECT_ID]: 15 };
    harness.pages = [makePageRow({ name: "Mine", owned_by: PAGES_USER_ID })];
    const view = renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Mine" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toHaveLength(6));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Make a copy",
      "Lock",
      "Make private",
      "Archive",
    ]);

    view.unmount();
    harness.pages = [makePageRow({ name: "Theirs", owned_by: "623e4567-e89b-12d3-a456-426614174005" })];
    renderScreen();
    await waitFor(() => expect(screen.getByRole("link", { name: "Theirs" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Theirs" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open in new tab",
      "Copy link",
      "Make a copy",
    ]);
  });

  it("gives a project admin the switches on someone else's page", async () => {
    harness.roles = { [PAGES_PROJECT_ID]: 20 };
    harness.pages = [makePageRow({ name: "Admin view", owned_by: "623e4567-e89b-12d3-a456-426614174005" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Admin view" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toContain("Make private");
  });

  it("makes the row private and re-reads the collection", async () => {
    harness.pages = [makePageRow({ name: "Switch me" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Switch me" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Make private" }));
    await waitFor(() => expect(harness.accessCalls).toBe(1));
    expect(toaster.notifySuccess).toHaveBeenCalledWith("Page marked private.");
  });

  it("locks and shelves a row through the same menu", async () => {
    harness.pages = [makePageRow({ name: "Stateful" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Stateful" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Lock" }));
    await waitFor(() => expect(harness.lockCalls).toBe(1));
    expect(toaster.notifySuccess).toHaveBeenCalledWith("Page locked successfully.");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Stateful" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    await waitFor(() => expect(harness.archiveCalls).toBe(1));
    expect(toaster.notifySuccess).toHaveBeenCalledWith("Page archived successfully.");
  });

  it("confirms deletion of a shelved row and cancels cleanly", async () => {
    harness.pages = [makePageRow({ name: "Shelved row", archived_at: "2026-10-01" })];
    renderScreen("archived");
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Shelved row" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "Delete page" })).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toHaveTextContent("Shelved row");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(harness.deleteCalls).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "Page actions for Shelved row" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => expect(harness.deleteCalls).toBe(1));
    expect(toaster.notifySuccess).toHaveBeenCalledWith("Page deleted successfully.");
  });

  it("offers no delete entry on a live row", async () => {
    harness.pages = [makePageRow({ name: "Live row" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for Live row" }));
    expect(screen.queryByRole("menuitem", { name: "Delete" })).not.toBeInTheDocument();
  });

  it("carries no move entry in the community build", async () => {
    harness.pages = [makePageRow({ name: "No move" })];
    renderScreen();
    await screen.findByTestId("pages-list");
    fireEvent.click(screen.getByRole("button", { name: "Page actions for No move" }));
    expect(screen.queryByRole("menuitem", { name: "Move to project…" })).not.toBeInTheDocument();
  });
});
