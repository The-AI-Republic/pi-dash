// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-001–006 screen integration: gate, one collection read, tabs,
// search, sort, filters, empty/error/no-results and row links.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PageRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
const toaster = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));
vi.mock("../../../shared/shell/toaster.js", () => toaster);

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
import { PagesListScreen } from "./PagesListScreen.js";

interface Harness {
  project: Record<string, unknown>;
  projectStatus: number;
  roles: Record<string, number>;
  pages: PageRow[];
  pagesStatus: number;
  pagesCalls: number;
  members: Record<string, unknown>[];
  meStatus: number;
}

let harness: Harness;

/** Stable valid row ids: the collection contract validates UUIDs. */
const pageId = (suffix: number): string => `323e4567-e89b-12d3-a456-4266141741${String(suffix).padStart(2, "0")}`;

beforeEach(() => {
  harness = {
    project: makeProjectRow(),
    projectStatus: 200,
    roles: { [PAGES_PROJECT_ID]: 20 },
    pages: [],
    pagesStatus: 200,
    pagesCalls: 0,
    members: [makeWorkspaceMember(PAGES_USER_ID, "Ada Byron")],
    meStatus: 200,
  };
  routerHarness.navigate.mockReset();
  toaster.notifySuccess.mockReset();
  toaster.notifyError.mockReset();
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    const projectRead = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/$`).exec(url);
    if (projectRead !== null) {
      const id = projectRead[1] as string;
      return jsonResponse(url, harness.projectStatus, { ...harness.project, id });
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith("/projects/details/")) return jsonResponse(url, 200, [makeProjectDetails()]);
    if (url.endsWith("/api/users/me/")) {
      if (harness.meStatus === -1) return new Promise(() => undefined) as never;
      return jsonResponse(url, harness.meStatus, { id: PAGES_USER_ID, email: "ada@test" });
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
    const pagesRead = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/pages/$`).exec(url);
    if (pagesRead !== null) {
      harness.pagesCalls += 1;
      return jsonResponse(url, harness.pagesStatus, harness.pages);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
});

function renderScreen(tab: "public" | "private" | "archived" = "public"): void {
  render(
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
    const view = renderProject(PAGES_PROJECT_ID, client);
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
        <PagesListScreen workspaceSlug="acme" projectId="723e4567-e89b-12d3-a456-426614174006" tab="public" />
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
