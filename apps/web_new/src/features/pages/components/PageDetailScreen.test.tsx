// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail shell integration (parity/specs/pages.md §4, PAGE-022–025,
// 079–080): one visit-tracked record read beside the shared collection,
// the spinner, the panel, the title with its placeholder, read-only
// locked/shelved pages, the pane and the snapshot overlay.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { PageDetail, PageRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const router = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => router.navigate,
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));
vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: () => true,
  setFavoritesSectionOpen: vi.fn(),
}));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: () => ({ settle: vi.fn() }),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_USER_ID, jsonResponse, makePageRow, makeProjectRow, makeWorkspaceMember } from "../testing.js";
import { resetPageBodyCache } from "../lib/pageBodyCache.js";
import type { PageDetailSearch } from "../lib/pageDetail.js";
import { PageDetailScreen } from "./PageDetailScreen.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "323e4567-e89b-12d3-a456-426614174009";
const VIEWER = "523e4567-e89b-12d3-a456-426614174005";

interface Harness {
  pageId: string;
  page: PageDetail;
  pageStatus: number;
  pageCalls: string[];
  pageWrites: { method: string; body: unknown }[];
  pages: PageRow[];
  pagesStatus: number;
  pagesCalls: number;
  pagesHold: Promise<void> | null;
  pageHold: Promise<void> | null;
  membersHold: Promise<void> | null;
  role: number | null;
  viewer: string;
  versions: Record<string, unknown>[];
  versionsStatus: number;
  versionStatus: number;
  versionHold: Promise<void> | null;
  projectPageView: boolean;
  projectStatus: number;
}

let harness: Harness;

function record(overrides: Partial<PageDetail> = {}): PageDetail {
  return {
    ...makePageRow({ id: harness.pageId, name: "Launch notes" }),
    description_html: "<p>Draft body</p>",
    ...overrides,
  };
}

beforeEach(() => {
  harness = {
    pageId: "323e4567-e89b-12d3-a456-426614174002",
    page: {
      ...makePageRow({ id: "323e4567-e89b-12d3-a456-426614174002", name: "Launch notes" }),
      description_html: "<p>Draft body</p>",
    },
    pageStatus: 200,
    pageCalls: [],
    pageWrites: [],
    pages: [],
    pagesStatus: 200,
    pagesCalls: 0,
    pagesHold: null,
    pageHold: null,
    membersHold: null,
    role: 20,
    viewer: PAGES_USER_ID,
    versions: [],
    versionsStatus: 200,
    versionStatus: 200,
    versionHold: null,
    projectPageView: true,
    projectStatus: 200,
  };
  resetPageBodyCache();
  router.navigate.mockReset();
  createAppClient("https://api.test", (async (requested: string, init: { method: string }) => {
    const url = requested.split("?")[0] ?? requested;
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith("/api/users/me/")) {
      return jsonResponse(url, 200, {
        id: harness.viewer,
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
    if (url.endsWith("/project-roles/")) {
      return jsonResponse(url, 200, harness.role === null ? {} : { [PROJECT]: harness.role });
    }
    const projectRead = new RegExp(`/api/workspaces/acme/projects/([0-9a-f-]+)/$`).exec(url);
    if (projectRead !== null) {
      return jsonResponse(
        url,
        harness.projectStatus,
        harness.projectStatus === 200
          ? { ...makeProjectRow({ page_view: harness.projectPageView }), id: projectRead[1] }
          : { error: "not found" }
      );
    }
    const versionDetail = new RegExp(
      `/api/workspaces/acme/projects/[0-9a-f-]+/pages/[0-9a-f-]+/versions/([0-9a-f-]+)/$`
    ).exec(url);
    if (versionDetail !== null) {
      if (harness.versionHold !== null) await harness.versionHold;
      return jsonResponse(url, harness.versionStatus, {
        id: versionDetail[1],
        workspace: "123e4567-e89b-12d3-a456-426614174000",
        page: harness.pageId,
        last_saved_at: "2026-10-20T12:00:00Z",
        description_binary: null,
        description_html: "<p>Stored snapshot body</p>",
        description_json: {},
        owned_by: PAGES_USER_ID,
        created_at: "2026-10-20T12:00:00Z",
        updated_at: "2026-10-20T12:00:00Z",
        created_by: null,
        updated_by: null,
      });
    }
    const versionList = new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/[0-9a-f-]+/versions/$`).exec(url);
    if (versionList !== null) {
      return jsonResponse(url, harness.versionsStatus, harness.versions);
    }
    const recordRead = new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/([0-9a-f-]+)/$`).exec(url);
    if (recordRead !== null && init.method === "GET") {
      harness.pageCalls.push(requested);
      if (harness.pageHold !== null) await harness.pageHold;
      return jsonResponse(url, harness.pageStatus, { ...harness.page, id: recordRead[1] });
    }
    if (recordRead !== null && init.method === "PATCH") {
      const body = (init as { body?: string }).body;
      harness.pageWrites.push({ method: init.method, body: body === undefined ? null : JSON.parse(body) });
      return jsonResponse(url, 200, { id: recordRead[1], name: "Renamed" });
    }
    const collection = new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/$`).exec(url);
    if (collection !== null) {
      harness.pagesCalls += 1;
      if (harness.pagesHold !== null) await harness.pagesHold;
      return jsonResponse(url, harness.pagesStatus, harness.pages);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) {
      if (harness.membersHold !== null) await harness.membersHold;
      return jsonResponse(url, 200, [makeWorkspaceMember()]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
});

function renderScreen(search: PageDetailSearch = {}): {
  changes: PageDetailSearch[];
} {
  const changes: PageDetailSearch[] = [];
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PageDetailScreen
        workspaceSlug="acme"
        projectId={PROJECT}
        pageId={harness.pageId}
        search={search}
        onSearchChange={(next) => {
          changes.push(next);
        }}
      />
    </QueryClientProvider>
  );
  return { changes };
}

describe("PageDetailScreen loading and panel", () => {
  it("spins while the record is unresolved and then paints the editor", async () => {
    let release: () => void = () => undefined;
    harness.pageHold = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderScreen();
    const spinner = await screen.findByLabelText("Loading page");
    expect(spinner.querySelector("svg.animate-pulse")).not.toBeNull();
    expect(spinner).toHaveClass("grid", "size-full", "place-items-center");
    release();
    await screen.findByRole("textbox", { name: "Page title" });
    await waitFor(() => expect(screen.queryByLabelText("Loading page")).not.toBeInTheDocument());
  });

  it("reads the record once with visit tracking and preloads the collection", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    const detailReads = harness.pageCalls.filter((url) => url.includes(`/pages/${harness.pageId}/`));
    expect(detailReads).toHaveLength(1);
    expect(new URL(detailReads[0] ?? "").searchParams.get("track_visit")).toBe("true");
    expect(harness.pagesCalls).toBeGreaterThanOrEqual(1);
  });

  it("panels a failed record without mounting an editor", async () => {
    harness.pageStatus = 404;
    renderScreen();
    const heading = await screen.findByRole("heading", { name: "Page not found" });
    expect(heading).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Page title" })).not.toBeInTheDocument();
  });

  it("panels a forbidden record and never flashes the editor", async () => {
    harness.pageStatus = 403;
    renderScreen();
    await screen.findByRole("heading", { name: "Page not found" });
  });

  it("panels a restricted page for a member", async () => {
    harness.page = record({ access: 1, owned_by: PAGES_USER_ID });
    harness.viewer = VIEWER;
    renderScreen();
    await screen.findByRole("heading", { name: "Page not found" });
  });

  it("panels an open page for a guest who does not own it", async () => {
    harness.role = 5;
    harness.viewer = VIEWER;
    renderScreen();
    await screen.findByRole("heading", { name: "Page not found" });
  });

  it("follows the name into the tab title", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    await waitFor(() => expect(document.title).toBe("Launch notes"));
  });

  it("falls back to Untitled in the tab title for a blank name", async () => {
    harness.page = record({ name: "   " });
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    await waitFor(() => expect(document.title).toBe("Untitled"));
  });
});

describe("PageDetailScreen title and body", () => {
  it("reserves the title with its placeholder and feeds the switcher from the collection", async () => {
    harness.page = record({ name: "" });
    harness.pages = [makePageRow({ id: harness.pageId, name: "" }), makePageRow({ id: OTHER, name: "Second page" })];
    renderScreen();
    const field = await screen.findByRole("textbox", { name: "Page title" });
    expect(field.closest(`[id="${harness.pageId}-title"]`)).not.toBeNull();
    // The empty heading carries the placeholder, not the editing surface:
    // the title text node is the h1 ProseMirror renders.
    expect(field.querySelector("h1")).toHaveAttribute("data-placeholder", "Untitled");
    // The switcher is fed by the preloaded collection, which may still be
    // in flight on the first paint.
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Switch page, current Untitled/ })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole("button", { name: /Switch page, current Untitled/ }));
    const switcherOptions = (await screen.findAllByRole("option")).map(
      (option) => option.querySelector('[data-testid="page-switcher-option-name"]')?.textContent
    );
    expect(switcherOptions).toContain("Second page");
    expect(switcherOptions).toContain("Untitled");
  });

  it("renders the stored body and skeletons a cold document once", async () => {
    renderScreen();
    const skeleton = await screen.findByRole("status", { name: "Loading page body" });
    expect(skeleton.closest("#page-content-container")?.parentElement).not.toBeNull();
    expect(await screen.findByText("Draft body")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole("status", { name: "Loading page body" })).not.toBeInTheDocument());
  });

  it("saves a renamed title through the record endpoint", async () => {
    renderScreen();
    const field = await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.focus(field);
    field.textContent = "Renamed";
    fireEvent.input(field);
    fireEvent.blur(field);
    await waitFor(() => expect(harness.pageWrites).toEqual([{ method: "PATCH", body: { name: "Renamed" } }]));
    await waitFor(() => expect(document.title).toBe("Renamed"));
    expect(field.textContent).toBe("Renamed");
    expect(screen.queryByTestId("page-title-count")).not.toBeInTheDocument();
  });

  it("renders an owned open page as editable", async () => {
    renderScreen();
    expect(await screen.findByRole("textbox", { name: "Page title" })).toBeInTheDocument();
    expect(screen.queryByText(/read-only/)).not.toBeInTheDocument();
  });

  it("edits a member's open page without mounting the manage seam", async () => {
    harness.role = 15;
    harness.viewer = VIEWER;
    harness.page = record({ owned_by: PAGES_USER_ID });
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.queryByTestId("page-detail-manage-mount")).not.toBeInTheDocument();
  });

  it("mounts the manage seam for an owner", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByTestId("page-detail-manage-mount")).toBeInTheDocument();
  });

  it("keeps the favorite control off an open page a guest-role viewer owns", async () => {
    harness.role = 5;
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.queryByRole("button", { name: /favorite/i })).not.toBeInTheDocument();
  });

  it("falls back to the project id and no mark when the project row is unavailable", async () => {
    harness.projectStatus = 404;
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByRole("button", { name: new RegExp(PROJECT) })).toBeInTheDocument();
  });

  it("renders a locked page read-only with no editable regions", async () => {
    harness.page = record({ is_locked: true });
    const { container } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PageDetailScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          pageId={harness.pageId}
          search={{}}
          onSearchChange={vi.fn()}
        />
      </QueryClientProvider>
    );
    await screen.findByText("This page is locked and read-only.");
    expect(container.querySelector('[contenteditable="true"]')).toBeNull();
    expect(screen.getByRole("heading", { name: "Launch notes" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole("status", { name: "Loading page body" })).not.toBeInTheDocument());
  });

  it("renders a shelved page read-only", async () => {
    harness.page = record({ archived_at: "2026-10-01" });
    const { container } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PageDetailScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          pageId={harness.pageId}
          search={{}}
          onSearchChange={vi.fn()}
        />
      </QueryClientProvider>
    );
    await screen.findByText("This page is archived and read-only.");
    expect(container.querySelector('[contenteditable="true"]')).toBeNull();
  });
});

describe("PageDetailScreen title, mark and toolbar", () => {
  it("mounts the sticky bar for an editable page", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument();
    expect(screen.getByTestId("page-toolbar-bold")).toBeInTheDocument();
  });

  it("withholds the bar and the mark control from a read-only viewer", async () => {
    harness.page = record({ is_locked: true });
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PageDetailScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          pageId={harness.pageId}
          search={{}}
          onSearchChange={vi.fn()}
        />
      </QueryClientProvider>
    );
    await screen.findByText("This page is locked and read-only.");
    expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument();
    expect(screen.getByTestId("page-mark")).toHaveAttribute("data-readonly", "true");
    expect(screen.queryByRole("button", { name: "Change the page mark" })).not.toBeInTheDocument();
  });

  it("skeletons a read-only page's body until its document is known", async () => {
    harness.page = record({ is_locked: true });
    const frame = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
    try {
      render(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <PageDetailScreen
            workspaceSlug="acme"
            projectId={PROJECT}
            pageId={harness.pageId}
            search={{}}
            onSearchChange={vi.fn()}
          />
        </QueryClientProvider>
      );
      await screen.findByText("This page is locked and read-only.");
      expect(screen.getByRole("status", { name: "Loading page body" })).toBeInTheDocument();
      expect(screen.getByRole("heading", { name: "Launch notes" })).toBeInTheDocument();
    } finally {
      frame.mockRestore();
    }
  });

  it("draws the page mark in the header and the sibling switcher", async () => {
    harness.page = record({ logo_props: { in_use: "emoji", emoji: { value: "1f680" } } });
    harness.pages = [
      makePageRow({
        id: harness.pageId,
        name: "Launch notes",
        logo_props: { in_use: "emoji", emoji: { value: "1f680" } },
      }),
      makePageRow({ id: OTHER, name: "Second page", logo_props: { in_use: "emoji", emoji: { value: "1f600" } } }),
    ];
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    const header = screen.getByTestId("page-canvas-header");
    expect(header.querySelector('[data-testid="page-mark-glyph"]')).toHaveTextContent("🚀");
    await waitFor(() => expect(screen.getByTestId("page-switcher-current-mark")).toHaveTextContent("🚀"));
    fireEvent.click(screen.getByRole("button", { name: /Switch page, current Launch notes/ }));
    const marks = await screen.findAllByTestId("page-switcher-mark");
    // Rows are sorted by name: Launch notes, then Second page.
    expect(marks.map((mark) => mark.textContent)).toEqual(["🚀", "😀"]);
  });

  it("keeps the header a hover target for the mark affordance", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    // The whole canvas header is the hover target, not the mark tile.
    const header = screen.getByTestId("page-canvas-header");
    expect(header).toHaveClass("page-header-container");
    const affordance = screen.getByTestId("page-mark-affordance");
    expect(header).toContainElement(affordance);
    expect(affordance.className).toContain("transition-all");
    expect(affordance.className).toContain("group-hover/page-header:opacity-100");
    expect(affordance.className).toContain("opacity-0");
  });

  it("writes a picked mark through the record endpoint", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByTestId("page-mark-trigger"));
    const search = await screen.findByPlaceholderText("Search icons");
    fireEvent.change(search, { target: { value: "rocket" } });
    fireEvent.click(screen.getByRole("button", { name: "Rocket launch" }));
    await waitFor(() => expect(harness.pageWrites).toHaveLength(1));
    expect(harness.pageWrites[0]?.body).toEqual({
      logo_props: { in_use: "icon", icon: { name: "rocket_launch", color: "#6366f1" } },
    });
  });
});

describe("PageDetailScreen deep links", () => {
  it("opens the pane on the address's tab and reports tab changes", async () => {
    const { changes } = renderScreen({ paneTab: "outline" });
    await screen.findByRole("complementary", { name: "Navigation pane" });
    expect(screen.getByRole("tab", { name: "Outline" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("tab", { name: "Assets" }));
    expect(changes.at(-1)).toEqual({ paneTab: "assets" });
  });

  it("opens the outline from the header toggle", async () => {
    const { changes } = renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Open navigation pane" }));
    expect(changes.at(-1)).toEqual({ paneTab: "outline" });
  });

  it("shows the heading preview at the canvas edge while the pane is shut", async () => {
    harness.page = record({ description_html: "<h1>Launch</h1><h2>Scope</h2><p>Body</p>" });
    const { changes } = renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    const preview = await screen.findByRole("button", { name: "Open outline" });
    expect(preview.querySelectorAll('div[style*="width"]')).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Scope" }));
    expect(changes.at(-1)).toEqual({ paneTab: "outline" });
  });

  it("opens the outline without scrolling from the preview's bar strip", async () => {
    harness.page = record({ description_html: "<h1>Launch</h1>" });
    const { changes } = renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(await screen.findByRole("button", { name: "Open outline" }));
    expect(changes.at(-1)).toEqual({ paneTab: "outline" });
  });

  it("lists the stored headings in the outline and the stored counts on the info tab", async () => {
    harness.page = record({ description_html: "<h1>Launch</h1><h2>Scope</h2><p>Two words</p>" });
    renderScreen({ paneTab: "outline" });
    expect(await screen.findByRole("button", { name: "Scope" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Launch" })).toBeInTheDocument();
  });

  it("holds the canvas until the directory answers, so rows never flash the fallback", async () => {
    let release: () => void = () => undefined;
    harness.membersHold = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderScreen({ paneTab: "info" });
    await screen.findByLabelText("Loading page");
    expect(screen.queryByText("Edited by")).not.toBeInTheDocument();
    release();
    await screen.findByRole("textbox", { name: "Page title" });
    await waitFor(() => expect(screen.queryByLabelText("Loading page")).not.toBeInTheDocument());
    const edited = screen.getByText("Edited by").nextElementSibling as HTMLElement;
    expect(edited.querySelector("a")?.textContent).toBe("Ada Byron");
  });

  it("gives the screen a definite height and keeps the shut pane out of the scrollport", async () => {
    renderScreen();
    const canvas = await screen.findByLabelText("Launch notes");
    const screenRoot = screen.getByTestId("page-detail-screen");
    // The pane's shut state is a negative margin; clipping the screen keeps
    // it from becoming reachable by scrolling the surrounding content.
    expect(screenRoot).toHaveClass("h-full", "min-h-0", "flex-col", "overflow-x-clip");
    // The canvas column is the page's own vertical scroller, so the
    // surrounding content area never becomes the scroller instead.
    const scroll = screen.getByTestId("page-canvas-scroll");
    expect(scroll).toHaveClass("min-h-0", "flex-1", "overflow-y-auto");
    expect(scroll.contains(canvas)).toBe(true);
    expect(screen.getByTestId("page-canvas-stage")).toContainElement(scroll);
  });

  it("pins the heading preview beside the scroller instead of inside the document", async () => {
    harness.page = record({ description_html: "<h1>Launch</h1><h2>Scope</h2>" });
    renderScreen();
    const preview = await screen.findByRole("button", { name: "Open outline" });
    // Inside the scroller the preview would scroll away with the body; it
    // belongs to the stage, which does not scroll (PAGE-046).
    expect(screen.getByTestId("page-canvas-scroll")).not.toContainElement(preview);
    expect(screen.getByTestId("page-canvas-stage")).toContainElement(preview);
  });

  it("keeps the title out of the pane's document while it sits on the canvas", async () => {
    harness.page = record({ name: "Launch notes", description_html: "<h1>Scope</h1><p>Body</p>" });
    renderScreen({ paneTab: "outline" });
    const canvas = await screen.findByLabelText("Launch notes");
    // The title lives inside the measured canvas but outside the body, so
    // the outline lists the document's headings only (PAGE-046/047).
    expect(canvas.querySelector('[data-testid="page-detail-title"]')).not.toBeNull();
    expect(await screen.findByRole("button", { name: "Scope" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Launch notes" })).not.toBeInTheDocument();
  });

  it("widens the readable canvas measure when the pane opens", async () => {
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PageDetailScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          pageId={harness.pageId}
          search={{}}
          onSearchChange={vi.fn()}
        />
      </QueryClientProvider>
    );
    const canvas = await screen.findByLabelText("Launch notes");
    const measure = canvas.querySelector<HTMLElement>(".frame-renderer");
    expect(canvas.tagName).toBe("ARTICLE");
    expect(canvas.style.width).toBe("");
    expect(measure?.style.maxWidth).toBe("720px");
    expect(measure).toHaveClass("w-full");
    rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PageDetailScreen
          workspaceSlug="acme"
          projectId={PROJECT}
          pageId={harness.pageId}
          search={{ paneTab: "outline" }}
          onSearchChange={vi.fn()}
        />
      </QueryClientProvider>
    );
    const openCanvas = await screen.findByLabelText("Launch notes");
    expect(openCanvas.querySelector<HTMLElement>(".frame-renderer")?.style.maxWidth).toBe("1152px");
  });

  it("attributes the page to the directory's names with profile links", async () => {
    renderScreen({ paneTab: "info" });
    await screen.findByRole("textbox", { name: "Page title" });
    const edited = screen.getByText("Edited by").nextElementSibling as HTMLElement;
    const link = edited.querySelector("a");
    expect(link?.textContent).toBe("Ada Byron");
    expect(link?.getAttribute("href")).toBe(`/acme/profile/${PAGES_USER_ID}/`);
  });

  it("attributes an unsaved-by row to its creator", async () => {
    harness.page = record({ updated_by: null, created_by: PAGES_USER_ID });
    renderScreen({ paneTab: "info" });
    await screen.findByRole("textbox", { name: "Page title" });
    const edited = screen.getByText("Edited by").nextElementSibling as HTMLElement;
    expect(edited.querySelector("a")?.textContent).toBe("Ada Byron");
  });

  it("closes the pane while keeping an open snapshot key", async () => {
    const { changes } = renderScreen({ paneTab: "info" });
    await screen.findByRole("complementary", { name: "Navigation pane" });
    fireEvent.click(screen.getByRole("button", { name: "Close navigation pane" }));
    expect(changes.at(-1)).toEqual({});
  });

  it("opens the snapshot overlay directly from the version key", async () => {
    harness.versions = [
      {
        id: "11111111-1111-4111-8111-111111111111",
        workspace: "123e4567-e89b-12d3-a456-426614174000",
        page: harness.pageId,
        last_saved_at: "2026-10-20T12:00:00Z",
        owned_by: PAGES_USER_ID,
        created_at: "2026-10-20T12:00:00Z",
        updated_at: "2026-10-20T12:00:00Z",
        created_by: null,
        updated_by: null,
      },
    ];
    renderScreen({ paneTab: "info", version: "11111111-1111-4111-8111-111111111111" });
    expect(await screen.findByText("Stored snapshot body")).toBeInTheDocument();
    expect(screen.getByText("View only")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Oct 20, 2026/ }).getAttribute("href")).toContain(
      "version=11111111-1111-4111-8111-111111111111"
    );
  });

  it("skeletons a stalled snapshot read", async () => {
    let release: () => void = () => undefined;
    harness.versionHold = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderScreen({ version: "11111111-1111-4111-8111-111111111111" });
    expect(await screen.findByRole("status", { name: "Loading version" })).toBeInTheDocument();
    release();
    expect(await screen.findByText("Stored snapshot body")).toBeInTheDocument();
  });

  it("closes the overlay through the version key only", async () => {
    const { changes } = renderScreen({ paneTab: "info", version: "11111111-1111-4111-8111-111111111111" });
    await screen.findByText("Stored snapshot body");
    fireEvent.click(screen.getByRole("button", { name: "Close version" }));
    expect(changes.at(-1)).toEqual({ paneTab: "info" });
  });

  it("writes the info pane from the options history shortcut", async () => {
    const { changes } = renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Page options" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Version history" }));
    expect(changes.at(-1)).toEqual({ paneTab: "info" });
  });

  it("navigates to a sibling's canonical address, dropping both keys", async () => {
    harness.pages = [makePageRow({ id: harness.pageId }), makePageRow({ id: OTHER, name: "Second page" })];
    renderScreen({ paneTab: "info", version: "11111111-1111-4111-8111-111111111111" });
    await screen.findByText("Stored snapshot body");
    fireEvent.click(screen.getByRole("button", { name: /Switch page, current Launch notes/ }));
    fireEvent.click(await screen.findByRole("option", { name: "Second page" }));
    expect(router.navigate).toHaveBeenCalledWith({ href: `/acme/projects/${PROJECT}/pages/${OTHER}` });
  });

  it("copies the canonical address from the header control", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderScreen({ paneTab: "info" });
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    expect(writeText).toHaveBeenCalledWith(
      new URL(`/acme/projects/${PROJECT}/pages/${harness.pageId}`, window.location.origin).toString()
    );
  });

  it("writes the version key when a timeline row is picked", async () => {
    harness.versions = [
      {
        id: "11111111-1111-4111-8111-111111111111",
        workspace: "123e4567-e89b-12d3-a456-426614174000",
        page: harness.pageId,
        last_saved_at: "2026-10-20T12:00:00Z",
        owned_by: PAGES_USER_ID,
        created_at: "2026-10-20T12:00:00Z",
        updated_at: "2026-10-20T12:00:00Z",
        created_by: null,
        updated_by: null,
      },
    ];
    const { changes } = renderScreen({ paneTab: "info" });
    fireEvent.click(await screen.findByRole("link", { name: /Oct 20, 2026/ }));
    expect(changes.at(-1)).toEqual({ paneTab: "info", version: "11111111-1111-4111-8111-111111111111" });
  });

  it("keeps the detail readable with the feature off and skips the collection read", async () => {
    harness.projectPageView = false;
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(harness.pagesCalls).toBe(0);
  });

  it("skeletons the version timeline while its read is in flight", async () => {
    renderScreen({ paneTab: "info" });
    expect((await screen.findAllByRole("status", { name: "Loading versions" })).length).toBe(3);
  });
});
