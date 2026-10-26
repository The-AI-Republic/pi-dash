// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived work-item detail (parity/specs/archives.md §3.4–3.5,
// ARCH-011–012). One archive-scoped read fills the banner, the short-key
// crumb and the locked detail composition; the header menu restores,
// deletes and copies the archived address, and a missing record shows
// the not-found state with a way back to the live list.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => mocks.navigate,
}));

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { authKeys } from "../../auth/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { ArchivedIssueDetail } from "./ArchivedIssueDetail.js";
import { archivedIssueQueryOptions } from "./archivedIssues.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "423e4567-e89b-12d3-a456-426614174003";
const USER = "123e4567-e89b-12d3-a456-426614174000";

function projectRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: USER,
    cycle_view: true,
    module_view: true,
    issue_views_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function archivedRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: ISSUE,
    name: "Old login form",
    state_id: null,
    sort_order: 1,
    completed_at: "2026-01-01T00:00:00Z",
    priority: "none",
    complexity_score: 0,
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    agent_executor: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: "2026-01-03",
    is_synced: false,
    description_html: "<p>Retired form</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

interface Answer {
  status: number;
  body: unknown;
}

interface RenderOptions {
  detail?: Answer;
  projects?: Record<string, unknown>[];
  holdProject?: boolean;
  holdDetail?: Promise<void>;
  failRestore?: boolean;
  failDelete?: boolean;
  /** Skip seeding the archived read so the transport answers it (loader, not-found). */
  unseededDetail?: boolean;
}

function menuEntry(name: string): HTMLElement {
  const entry = screen
    .getAllByRole("menuitem", { hidden: true })
    .find((item) => (item.textContent ?? "").includes(name));
  if (entry === undefined) throw new Error(`expected the ${name} menu entry`);
  return entry;
}

function renderDetail(options: RenderOptions = {}) {
  const reads: string[] = [];
  const writes: Array<{ url: string; method: string }> = [];
  const apiClient = createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    const method = init.method ?? "GET";
    if (method === "GET") reads.push(url);
    else writes.push({ url, method });
    const reply = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return reply(200, { csrf_token: "csrf" });
    if (url.endsWith("/api/users/me/")) {
      return reply(200, {
        id: USER,
        email: "ada@example.test",
        username: "ada",
        first_name: "Ada",
        last_name: "L",
        display_name: "Ada L",
        avatar: "",
        avatar_url: null,
        cover_image: null,
        cover_image_url: null,
        date_joined: "2026-01-01T00:00:00Z",
        user_timezone: "UTC",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        is_password_autoset: false,
        last_login_medium: "email",
        last_login_time: null,
      });
    }
    if (method !== "GET") {
      if (url.includes(`/issues/${ISSUE}/archive/`)) {
        return options.failRestore === true ? reply(500, {}) : reply(204, {});
      }
      if (url.includes(`/issues/${ISSUE}/`)) {
        return options.failDelete === true ? reply(500, {}) : reply(204, {});
      }
      return reply(200, {});
    }
    if (url.includes(`/issues/${ISSUE}/archive/`)) {
      if (options.holdDetail !== undefined) await options.holdDetail;
      const detail = options.detail ?? { status: 200, body: archivedRow() };
      return reply(detail.status, detail.body);
    }
    if (options.holdProject === true && url.includes(`/projects/${PROJECT}`)) {
      return new Promise<never>(() => {});
    }
    if (new RegExp(`/projects/${PROJECT}/$`).test(new URL(url).pathname)) {
      return reply(200, options.projects?.[0] ?? projectRow());
    }
    if (new URL(url).pathname.endsWith("/projects/")) {
      return reply(200, options.projects ?? [projectRow()]);
    }
    if (
      url.includes("/sub-issues/") ||
      url.includes("/issue-relation/") ||
      url.includes("description-versions") ||
      url.includes("/issue-links/") ||
      url.includes("/attachments/") ||
      url.includes("/code-reviews/") ||
      url.includes("/history/") ||
      url.includes("/reactions/") ||
      url.endsWith("/comments/") ||
      url.includes("/states/")
    ) {
      return reply(200, url.includes("/sub-issues/") ? { sub_issues: [] } : []);
    }
    return reply(404, {});
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  const projects = options.projects ?? [projectRow()];
  if (options.holdProject !== true) {
    queryClient.setQueryData(authKeys.project(SLUG, PROJECT), projects[0] as never);
  }
  queryClient.setQueryData(projectsQueryOptions(apiClient, SLUG).queryKey, projects as never);
  if (options.unseededDetail !== true && options.holdDetail === undefined) {
    queryClient.setQueryData(
      archivedIssueQueryOptions(apiClient, SLUG, PROJECT, ISSUE).queryKey,
      archivedRow() as never
    );
  }
  const rootRoute = createRootRoute({
    component: () => (
      <QueryClientProvider client={queryClient}>
        <ArchivedIssueDetail workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} />
      </QueryClientProvider>
    ),
  });
  const liveRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/projects/$projectId/issues",
    component: () => null,
  });
  const archivedRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/projects/$projectId/archives/issues/",
    component: () => null,
  });
  const homeRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null });
  const router = createRouter({
    routeTree: rootRoute.addChildren([homeRoute, liveRoute, archivedRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(<RouterProvider router={router} />);
  return { reads, writes };
}

const clipboard = { writeText: vi.fn() };

beforeEach(() => {
  vi.clearAllMocks();
  clipboard.writeText.mockResolvedValue(undefined);
  Object.defineProperty(globalThis.navigator, "clipboard", { value: clipboard, configurable: true });
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("ArchivedIssueDetail", () => {
  it("renders the banner, short-key crumb and locked body from one read (ARCH-011)", async () => {
    const { reads } = renderDetail({ unseededDetail: true });
    const banner = await screen.findByTestId("archived-detail-banner");
    expect(banner).toHaveTextContent(/archived/);
    const trail = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(trail).getByText("WEB-7")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Old login form" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Title" })).not.toBeInTheDocument();
    expect(document.title).toBe("Old login form");
    const recordReads = reads.filter((url) => url.includes(`/issues/${ISSUE}/archive/`));
    expect(recordReads).toHaveLength(1);
  });

  it("links the banner back to the archived list (ARCH-011)", async () => {
    renderDetail();
    const banner = await screen.findByTestId("archived-detail-banner");
    const back = banner.querySelector("a");
    expect(back?.getAttribute("href")).toBe(`/${SLUG}/projects/${PROJECT}/archives/issues`);
  });

  it("shows the loader with no banner, activity or short key while fetching (ARCH-011)", async () => {
    let release: () => void = () => {};
    const holdDetail = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderDetail({ holdDetail });
    expect(await screen.findByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    expect(screen.queryByTestId("archived-detail-banner")).not.toBeInTheDocument();
    expect(screen.queryByText("WEB-7")).not.toBeInTheDocument();
    release();
    expect(await screen.findByTestId("archived-detail-banner")).toBeInTheDocument();
  });

  it("shows the not-found state with a way back to the live list (ARCH-011)", async () => {
    renderDetail({ detail: { status: 404, body: {} }, unseededDetail: true });
    expect(await screen.findByRole("heading", { name: "No work item matches this address" })).toBeInTheDocument();
    expect(screen.queryByTestId("archived-detail-banner")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "View live work items" })).toHaveAttribute(
      "href",
      `/${SLUG}/projects/${PROJECT}/issues`
    );
  });

  it("renders nothing until the project read resolves", async () => {
    renderDetail({ holdProject: true });
    await waitFor(() => expect(document.title).toBe("Old login form"));
    expect(screen.queryByTestId("archives-detail-frame")).not.toBeInTheDocument();
  });

  it("offers the archived menu with restore, open, copy and delete (ARCH-012)", async () => {
    renderDetail();
    await screen.findByTestId("archived-detail-banner");
    fireEvent.click(screen.getByTestId("detail-menu-trigger"));
    const titles = screen
      .getAllByRole("menuitem", { hidden: true })
      .map((item) => item.textContent ?? "")
      .filter((title) => title !== "");
    expect(titles).toEqual(["Restore", "Open in new tab", "Copy link", "Delete"]);
  });

  it("restores from the detail menu and lands on the live short-link (ARCH-012)", async () => {
    const { writes } = renderDetail();
    await screen.findByTestId("archived-detail-banner");
    fireEvent.click(screen.getByTestId("detail-menu-trigger"));
    fireEvent.click(menuEntry("Restore"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Restore success", expect.anything()));
    expect(writes).toContainEqual({
      url: expect.stringContaining(`/projects/${PROJECT}/issues/${ISSUE}/archive/`),
      method: "DELETE",
    });
    expect(mocks.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/issues/$issueId",
      params: { ws: SLUG, projectId: PROJECT, issueId: ISSUE },
    });
  });

  it("keeps a failed restore on the detail with an error confirmation (ARCH-012)", async () => {
    renderDetail({ failRestore: true });
    await screen.findByTestId("archived-detail-banner");
    fireEvent.click(screen.getByTestId("detail-menu-trigger"));
    fireEvent.click(menuEntry("Restore"));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(mocks.navigate).not.toHaveBeenCalled();
    expect(screen.getByTestId("archived-detail-banner")).toBeInTheDocument();
  });

  it("deletes from the detail menu with confirmation and returns to the list (ARCH-012)", async () => {
    const { writes } = renderDetail();
    await screen.findByTestId("archived-detail-banner");
    fireEvent.click(screen.getByTestId("detail-menu-trigger"));
    fireEvent.click(menuEntry("Delete"));
    expect(await screen.findByRole("heading", { name: "Delete work item" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete", exact: true }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Success!", expect.anything()));
    expect(writes).toContainEqual({
      url: expect.stringContaining(`/projects/${PROJECT}/issues/${ISSUE}/`),
      method: "DELETE",
    });
    expect(mocks.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/archives/issues/",
      params: { ws: SLUG, projectId: PROJECT },
    });
  });

  it("copies the archived address from the detail menu (ARCH-010)", async () => {
    renderDetail();
    await screen.findByTestId("archived-detail-banner");
    fireEvent.click(screen.getByTestId("detail-menu-trigger"));
    fireEvent.click(menuEntry("Copy link"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Link copied", undefined));
    expect(clipboard.writeText).toHaveBeenCalledWith(
      expect.stringContaining(`/${SLUG}/projects/${PROJECT}/archives/issues/${ISSUE}`)
    );
  });

  it("shows viewers only the copy-link entry (ARCH-012)", async () => {
    renderDetail({ projects: [projectRow({ member_role: 5 })] });
    await screen.findByTestId("archived-detail-banner");
    fireEvent.click(screen.getByTestId("detail-menu-trigger"));
    const titles = screen
      .getAllByRole("menuitem", { hidden: true })
      .map((item) => item.textContent ?? "")
      .filter((title) => title !== "");
    expect(titles).toEqual(["Open in new tab", "Copy link"]);
  });
});
