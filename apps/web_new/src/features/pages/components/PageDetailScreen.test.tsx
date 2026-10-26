// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail shell integration (parity/specs/pages.md §4, PAGE-022–025,
// 079–080): one visit-tracked record read beside the shared collection,
// the spinner, the panel, the title with its placeholder, read-only
// locked/shelved pages, the pane, the snapshot overlay and the restore
// that writes a snapshot back into the live editor (PAGE-053).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
// The restore write stays the real one; the spy only lets a test choose
// the refused answer, which a mounted editor in jsdom cannot produce.
vi.mock("../lib/pageRestore.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/pageRestore.js")>();
  return { ...actual, applyPageRestore: vi.fn(actual.applyPageRestore) };
});
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGES_USER_ID, jsonResponse, makePageRow, makeProjectRow, makeWorkspaceMember } from "../testing.js";
import { resetPageBodyCache } from "../lib/pageBodyCache.js";
import type { PageDetailSearch } from "../lib/pageDetail.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { getPageBodyDraft } from "../lib/pageBodyDraft.js";
import { applyPageRestore } from "../lib/pageRestore.js";
import { PageDetailScreen } from "./PageDetailScreen.js";
import { mentionNameFor, resetMentionDirectory } from "../../../shared/editor/mentionDirectory.js";
import { requestEntityPeek, resetEntityPeek } from "../../../shared/shell/entityPeek.js";

const issuePeek = vi.hoisted(() => vi.fn());
vi.mock("../../issues/peek/ResolvingIssuePeek.js", () => ({
  ResolvingIssuePeek: (props: { peeked: string; peekProjectId?: string | null; onClose: () => void }) => {
    issuePeek(props);
    return (
      <button type="button" onClick={props.onClose}>
        Close issue peek
      </button>
    );
  },
}));

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "323e4567-e89b-12d3-a456-426614174009";
const MID = "423e4567-e89b-12d3-a456-426614174010";
const VIEWER = "523e4567-e89b-12d3-a456-426614174005";

interface Harness {
  pageId: string;
  page: PageDetail;
  pageStatus: number;
  pageCalls: string[];
  pageWrites: { method: string; body: unknown }[];
  pages: PageRow[];
  childrenByParent: Record<string, PageRow[]>;
  pagesStatus: number;
  pagesCalls: number;
  pagesHold: Promise<void> | null;
  pageHold: Promise<void> | null;
  membersHold: Promise<void> | null;
  createdId: string;
  createBodies: Record<string, unknown>[];
  role: number | null;
  viewer: string;
  versions: Record<string, unknown>[];
  versionsStatus: number;
  versionsCalls: number;
  versionStatus: number;
  versionHold: Promise<void> | null;
  versionDetailCalls: number;
  versionJson: Record<string, unknown>;
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
  resetMentionDirectory();
  resetEntityPeek();
  issuePeek.mockReset();
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
    childrenByParent: {},
    pagesStatus: 200,
    pagesCalls: 0,
    pagesHold: null,
    pageHold: null,
    membersHold: null,
    createdId: "423e4567-e89b-12d3-a456-426614174077",
    createBodies: [],
    role: 20,
    viewer: PAGES_USER_ID,
    versions: [],
    versionsStatus: 200,
    versionsCalls: 0,
    versionStatus: 200,
    versionHold: null,
    versionDetailCalls: 0,
    versionJson: {},
    projectPageView: true,
    projectStatus: 200,
  };
  resetPageBodyCache();
  router.navigate.mockReset();
  createAppClient("https://api.test", (async (requested: string, init: { method: string; body?: string | null }) => {
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
      harness.versionDetailCalls += 1;
      if (harness.versionHold !== null) await harness.versionHold;
      return jsonResponse(url, harness.versionStatus, {
        id: versionDetail[1],
        workspace: "123e4567-e89b-12d3-a456-426614174000",
        page: harness.pageId,
        last_saved_at: "2026-10-20T12:00:00Z",
        description_binary: null,
        description_html: "<p>Stored snapshot body</p>",
        description_json: harness.versionJson,
        owned_by: PAGES_USER_ID,
        created_at: "2026-10-20T12:00:00Z",
        updated_at: "2026-10-20T12:00:00Z",
        created_by: null,
        updated_by: null,
      });
    }
    const versionList = new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/[0-9a-f-]+/versions/$`).exec(url);
    if (versionList !== null) {
      harness.versionsCalls += 1;
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
    if (url.endsWith("/duplicate/") && init.method === "POST") {
      harness.pageWrites.push({ method: "DUPLICATE", body: null });
      return jsonResponse(url, 200, {
        ...makePageRow({ id: harness.createdId, name: `Copy of ${harness.page.name}` }),
        description_html: "",
      });
    }
    if (url.endsWith("/access/") && init.method === "POST") {
      const body = (init as { body?: string }).body;
      const parsed = (body === undefined ? null : JSON.parse(body)) as { access?: number } | null;
      harness.pageWrites.push({ method: "ACCESS", body: parsed });
      if (typeof parsed?.access === "number") harness.page = { ...harness.page, access: parsed.access };
      return jsonResponse(url, 204, null);
    }
    if (url.endsWith("/lock/")) {
      const unlocking = init.method === "DELETE";
      harness.pageWrites.push({ method: unlocking ? "UNLOCK" : "LOCK", body: null });
      harness.page = { ...harness.page, is_locked: !unlocking };
      return jsonResponse(url, 204, null);
    }
    if (url.endsWith("/archive/")) {
      const restoring = init.method === "DELETE";
      harness.pageWrites.push({ method: restoring ? "RESTORE" : "ARCHIVE", body: null });
      harness.page = restoring
        ? { ...harness.page, archived_at: null }
        : { ...harness.page, archived_at: "2026-10-25", is_favorite: false };
      return jsonResponse(url, 200, { archived_at: "2026-10-25 10:00:00.123456" });
    }
    if (recordRead !== null && init.method === "DELETE") {
      harness.pageWrites.push({ method: "DELETE", body: null });
      harness.pageStatus = 404;
      return jsonResponse(url, 204, null);
    }
    const collection = new RegExp(`/api/workspaces/acme/projects/[0-9a-f-]+/pages/$`).exec(url);
    if (collection !== null && init.method === "POST") {
      harness.createBodies.push(JSON.parse(init.body ?? "{}") as Record<string, unknown>);
      return jsonResponse(url, 201, {
        ...makePageRow({ id: harness.createdId, name: "Untitled", parent: harness.pageId }),
        parent_id: harness.pageId,
        description_html: "",
      });
    }
    if (collection !== null) {
      harness.pagesCalls += 1;
      if (harness.pagesHold !== null) await harness.pagesHold;
      const parent = new URL(requested).searchParams.get("parent");
      if (parent !== null) return jsonResponse(url, harness.pagesStatus, harness.childrenByParent[parent] ?? []);
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
  resetMentionDirectory();
  resetEntityPeek();
});

afterEach(() => {
  cleanup();
  resetAppClient();
  document.title = "";
  vi.mocked(applyPageRestore).mockClear();
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
  window.localStorage.clear();
});

function renderScreen(search: PageDetailSearch = {}): {
  changes: PageDetailSearch[];
} {
  const changes: PageDetailSearch[] = [];
  // Hold the client for the mounted tree's lifetime: unmounting stops
  // pending retries, and clearing the query cache releases their timers
  // before the next test resets the app client.
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
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

const SNAPSHOT_ROW = {
  id: "11111111-1111-4111-8111-111111111111",
  workspace: "123e4567-e89b-12d3-a456-426614174000",
  page: "33333333-3333-4333-8333-333333333333",
  last_saved_at: "2026-10-20T12:00:00Z",
  owned_by: PAGES_USER_ID,
  created_at: "2026-10-20T12:00:00Z",
  updated_at: "2026-10-20T12:00:00Z",
  created_by: null,
  updated_by: null,
};

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

  it("skips the ancestor forest read for a top-level page", async () => {
    harness.pages = [];
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(harness.pagesCalls).toBe(1);
  });

  it("skips the ancestor forest read when the pages feature is off", async () => {
    harness.page = record({ parent: OTHER, parent_id: OTHER });
    harness.projectPageView = false;
    renderScreen();
    // With the feature off the shell still paints the record, but the
    // breadcrumb walk is gated with it.
    await screen.findByRole("textbox", { name: "Page title" });
    expect(harness.pagesCalls).toBe(0);
  });

  it("resolves an archived middle ancestor under a live top-level parent", async () => {
    // `sub_pages_count` hides the shelved middle page from a count-driven
    // walk (§11.7), so the trail would stop at the restricted fallback; the
    // deep walk reaches it and every ancestor links (PAGE-086).
    const liveTop = makePageRow({ id: OTHER, name: "Handbook", sub_pages_count: 0 });
    const middle = makePageRow({
      id: MID,
      name: "Policies",
      parent: OTHER,
      parent_id: OTHER,
      archived_at: "2026-10-20",
      sub_pages_count: 0,
    });
    harness.page = record({
      name: "Leave",
      parent: MID,
      parent_id: MID,
      archived_at: "2026-10-20",
    });
    harness.pages = [liveTop];
    harness.childrenByParent = { [OTHER]: [middle], [MID]: [harness.page] };
    renderScreen();
    const trail = await screen.findByRole("navigation", { name: "Page path" });
    expect(within(trail).getByRole("link", { name: "Handbook" })).toHaveAttribute(
      "href",
      `/acme/projects/${PROJECT}/pages/${OTHER}`
    );
    expect(within(trail).getByRole("link", { name: "Policies" })).toBeInTheDocument();
    expect(within(trail).queryByText("Restricted page")).not.toBeInTheDocument();
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
    // The title mounts behind a Suspense boundary whose fallback heading
    // answers to the same role and name as the lazy editor: under load
    // the found node can be swapped out and detached before the next line
    // runs, so both reads below re-query the live node inside one retry
    // instead of asserting on a stale reference (NEWFRONT-909).
    await screen.findByRole("textbox", { name: "Page title" });
    await waitFor(() => {
      const live = screen.getByRole("textbox", { name: "Page title" });
      expect(live.closest(`[id="${harness.pageId}-title"]`)).not.toBeNull();
      // The empty heading carries the placeholder, not the editing surface:
      // the title text node is the h1 ProseMirror renders.
      expect(live.querySelector("h1")).toHaveAttribute("data-placeholder", "Untitled");
    });
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

  it("resolves a stored picture's asset id on a read-only body", async () => {
    harness.page = record({
      is_locked: true,
      description_html: '<p><image-component src="asset-4" status="uploaded"></image-component></p>',
    });
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
    await waitFor(() => {
      const image = document.querySelector(".editor-container img");
      expect(image?.getAttribute("src")).toBe("/api/assets/v2/workspaces/acme/projects/" + PROJECT + "/asset-4/");
    });
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
    fireEvent.click(screen.getByRole("button", { name: "Page options" }));
    expect(await screen.findByRole("menuitem", { name: "Version history" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Add sub-page" })).not.toBeInTheDocument();
  });
});

describe("PageDetailScreen title, mark and toolbar", () => {
  it("mounts the sticky bar for an editable page", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument();
    expect(screen.getByTestId("page-toolbar-bold")).toBeInTheDocument();
  });

  it("hides the bar when the sticky preference is off and keeps the floating pane toggle", async () => {
    window.localStorage.setItem("pages_config", JSON.stringify({ sticky_toolbar: false }));
    const { changes } = renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument();
    expect(screen.getByTestId("page-floating-pane-toggle")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open navigation pane" }));
    expect(changes.at(-1)).toEqual({ paneTab: "outline" });
  });

  it("keeps the bar, and no floating toggle, when the sticky preference is on", async () => {
    window.localStorage.setItem("pages_config", JSON.stringify({ sticky_toolbar: true }));
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument();
    expect(screen.queryByTestId("page-floating-pane-toggle")).not.toBeInTheDocument();
  });

  it("leaves the floating toggle to the header control while the pane is open", async () => {
    window.localStorage.setItem("pages_config", JSON.stringify({ sticky_toolbar: false }));
    renderScreen({ paneTab: "outline" });
    await screen.findByRole("complementary", { name: "Navigation pane" });
    expect(screen.queryByTestId("page-floating-pane-toggle")).not.toBeInTheDocument();
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
    expect(screen.getByTestId("page-floating-pane-toggle")).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("button", { name: "Navigation pane" }));
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

  it("creates a sub-page from the header menu with the open page as parent and opens it", async () => {
    const { changes } = renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Page options" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Add sub-page" }));
    await waitFor(() => expect(harness.createBodies).toHaveLength(1));
    expect(harness.createBodies[0]).toMatchObject({ parent: harness.pageId, name: "Untitled", access: 0 });
    await waitFor(() =>
      expect(router.navigate).toHaveBeenCalledWith({
        href: `/acme/projects/${PROJECT}/pages/${harness.createdId}`,
      })
    );
    expect(changes).toEqual([]);
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

  it("names the snapshot's author in the timeline", async () => {
    harness.versions = [SNAPSHOT_ROW];
    renderScreen({ paneTab: "info" });
    const link = await screen.findByRole("link", { name: /Oct 20, 2026/ });
    await waitFor(() => expect(link).toHaveTextContent("Ada Byron"));
  });

  it("prefers the structured snapshot content over the stored markup", async () => {
    harness.versionJson = {
      type: "doc",
      content: [{ type: "paragraph", content: [{ type: "text", text: "Structured snapshot body" }] }],
    };
    renderScreen({ version: SNAPSHOT_ROW.id });
    expect(await screen.findByText("Structured snapshot body")).toBeInTheDocument();
    expect(screen.queryByText("Stored snapshot body")).not.toBeInTheDocument();
  });

  it("shows the error panel when the snapshot read fails", async () => {
    harness.versionStatus = 500;
    renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    expect(await screen.findByRole("heading", { name: "Something went wrong!" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    const overlay = screen.getByRole("dialog", { name: "Version snapshot" });
    expect(within(overlay).getByRole("heading", { level: 6 })).toHaveTextContent("Loading version details");
  });

  it("refetches the snapshot from the error panel's retry", async () => {
    harness.versionStatus = 500;
    renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    await screen.findByRole("heading", { name: "Something went wrong!" });
    const before = harness.versionDetailCalls;
    harness.versionStatus = 200;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Stored snapshot body")).toBeInTheDocument();
    expect(harness.versionDetailCalls).toBeGreaterThan(before);
  });

  it("leaves the snapshot from the timeline's current row", async () => {
    harness.versions = [SNAPSHOT_ROW];
    const { changes } = renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    await screen.findByText("Stored snapshot body");
    fireEvent.click(await screen.findByRole("link", { name: "Current version" }));
    expect(changes.at(-1)).toEqual({ paneTab: "info" });
  });

  it("restores the snapshot into the live editor in place and closes the overlay", async () => {
    harness.versions = [SNAPSHOT_ROW];
    harness.versionJson = {
      type: "doc",
      content: [{ type: "paragraph", content: [{ type: "text", text: "Snapshot words" }] }],
    };
    const { changes } = renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    const editor = await screen.findByLabelText("Page body");
    await waitFor(() => expect(editor).toHaveAttribute("contenteditable", "true"));
    const beforeList = harness.versionsCalls;
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() => expect(editor).toHaveTextContent("Snapshot words"));
    expect(editor).not.toHaveTextContent("Draft body");
    await waitFor(() => expect(getPageBodyDraft(harness.pageId)).toContain("Snapshot words"));
    expect(changes.at(-1)).toEqual({ paneTab: "info" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Page version restored."));
    await waitFor(() => expect(harness.versionsCalls).toBeGreaterThan(beforeList));
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("falls back to the stored markup when the snapshot has no known structure", async () => {
    harness.versions = [SNAPSHOT_ROW];
    renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    const editor = await screen.findByLabelText("Page body");
    await waitFor(() => expect(editor).toHaveAttribute("contenteditable", "true"));
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() => expect(editor).toHaveTextContent("Stored snapshot body"));
  });

  it("restores the editor's own formatting: underline and alignment survive the write", async () => {
    harness.versions = [SNAPSHOT_ROW];
    harness.versionJson = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          attrs: { textAlign: "center" },
          content: [{ type: "text", marks: [{ type: "underline" }], text: "Aligned underline" }],
        },
      ],
    };
    renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    const body = await screen.findByLabelText("Version body");
    await waitFor(() => expect(body.querySelector("u")).toHaveTextContent("Aligned underline"));
    expect(body.querySelector("p")).toHaveStyle({ textAlign: "center" });
    const editor = await screen.findByLabelText("Page body");
    await waitFor(() => expect(editor).toHaveAttribute("contenteditable", "true"));
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() => expect(editor.querySelector("u")).toHaveTextContent("Aligned underline"));
    expect(editor.querySelector("p")).toHaveStyle({ textAlign: "center" });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Page version restored."));
  });

  it("keeps a snapshot link's stored target and rel through display and restore", async () => {
    harness.versions = [SNAPSHOT_ROW];
    harness.versionJson = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          attrs: { textAlign: null },
          content: [
            {
              type: "text",
              marks: [
                {
                  type: "link",
                  attrs: {
                    href: "https://example.test/linked",
                    target: "_blank",
                    rel: "noopener noreferrer nofollow",
                    class: null,
                  },
                },
              ],
              text: "Linked words",
            },
          ],
        },
      ],
    };
    renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    const body = await screen.findByLabelText("Version body");
    await waitFor(() => expect(body.querySelector("a")).toHaveTextContent("Linked words"));
    const shown = body.querySelector("a");
    expect(shown?.getAttribute("href")).toBe("https://example.test/linked");
    expect(shown?.getAttribute("target")).toBe("_blank");
    expect(shown?.getAttribute("rel")).toBe("noopener noreferrer nofollow");
    const editor = await screen.findByLabelText("Page body");
    await waitFor(() => expect(editor).toHaveAttribute("contenteditable", "true"));
    fireEvent.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() => expect(editor.querySelector("a")).toHaveTextContent("Linked words"));
    const restored = editor.querySelector("a");
    expect(restored?.getAttribute("target")).toBe("_blank");
    expect(restored?.getAttribute("rel")).toBe("noopener noreferrer nofollow");
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Page version restored."));
  });

  it("reports a refused restore and leaves the overlay and address alone", async () => {
    harness.versions = [SNAPSHOT_ROW];
    vi.mocked(applyPageRestore).mockReturnValueOnce(false);
    const { changes } = renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    await screen.findByText("Stored snapshot body");
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(notifyError).toHaveBeenCalledWith("Page version could not be restored.");
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(changes).toEqual([]);
    expect(screen.getByRole("dialog", { name: "Version snapshot" })).toBeInTheDocument();
  });

  it("never offers restore to a read-only viewer", async () => {
    harness.versions = [SNAPSHOT_ROW];
    harness.page = record({ is_locked: true });
    renderScreen({ paneTab: "info", version: SNAPSHOT_ROW.id });
    expect(await screen.findByText("Stored snapshot body")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
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

describe("PageDetailScreen display preferences (PAGE-072)", () => {
  async function openOptions(): Promise<HTMLElement[]> {
    fireEvent.click(screen.getByRole("button", { name: "Page options" }));
    return screen.findAllByRole("menuitem");
  }

  it("toggles full width, reflows the canvas immediately and stores it locally", async () => {
    renderScreen();
    const canvas = await screen.findByLabelText("Launch notes");
    const measure = canvas.querySelector<HTMLElement>(".frame-renderer");
    expect(measure?.style.maxWidth).toBe("720px");
    expect(screen.getByTestId("page-formatting-bar").querySelector(".page-toolbar-content")).not.toHaveClass(
      "wide-layout"
    );

    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Full width" }));
    await waitFor(() => expect(measure?.style.maxWidth).toBe("1152px"));
    expect(screen.getByTestId("page-formatting-bar").querySelector(".page-toolbar-content")).toHaveClass("wide-layout");
    expect(JSON.parse(window.localStorage.getItem("pages_config") ?? "{}")).toMatchObject({ full_width: true });
    // Local only: the preference never reached the record endpoint.
    expect(harness.pageWrites).toHaveLength(0);
  });

  it("toggles the sticky toolbar off and on without hiding the pane control", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument();

    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Sticky toolbar" }));
    await waitFor(() => expect(screen.queryByRole("toolbar", { name: "Formatting" })).not.toBeInTheDocument());
    expect(JSON.parse(window.localStorage.getItem("pages_config") ?? "{}")).toMatchObject({ sticky_toolbar: false });

    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Sticky toolbar" }));
    await waitFor(() => expect(screen.getByRole("toolbar", { name: "Formatting" })).toBeInTheDocument());
  });

  it("applies the stored type size and face on mount, with no switcher in the menu", async () => {
    window.localStorage.setItem(
      "pages_config",
      JSON.stringify({ full_width: false, font_size: "small-font", font_style: "serif", sticky_toolbar: true })
    );
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-small-font", "text-face-serif");
    const items = (await openOptions()).map((item) => item.textContent ?? "");
    expect(items.some((item) => /font|typeface|typography/i.test(item))).toBe(false);
    // The preference never rewrote the page record.
    expect(harness.pageWrites).toHaveLength(0);
  });

  it("applies the stored type size to a read-only title and body", async () => {
    window.localStorage.setItem("pages_config", JSON.stringify({ font_size: "small-font", font_style: "serif" }));
    harness.page = record({ is_locked: true });
    renderScreen();
    await screen.findByText("This page is locked and read-only.");
    expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-small-font", "text-face-serif");
    await waitFor(() =>
      expect(document.querySelector(".editor-container")).toHaveClass("text-size-small-font", "text-face-serif")
    );

    cleanup();
    window.localStorage.clear();
    renderScreen();
    await screen.findByText("This page is locked and read-only.");
    expect(screen.getByTestId("page-detail-title")).toHaveClass("text-size-large-font", "text-face-sans-serif");
  });

  it("keeps the stored preferences across a remount (reload)", async () => {
    const first = renderScreen();
    void first;
    await screen.findByRole("textbox", { name: "Page title" });
    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Full width" }));
    await waitFor(() => expect(window.localStorage.getItem("pages_config")).toContain('"full_width":true'));
    cleanup();
    renderScreen();
    const canvas = await screen.findByLabelText("Launch notes");
    await waitFor(() => expect(canvas.querySelector<HTMLElement>(".frame-renderer")?.style.maxWidth).toBe("1152px"));
  });
});

describe("PageDetailScreen negative rows (PAGE-082, PAGE-083)", () => {
  it("carries a mark but no cover affordance and no resting file input", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.getByTestId("page-mark-affordance")).toBeInTheDocument();
    expect(screen.queryByText(/cover image|add cover|page cover|change cover/i)).not.toBeInTheDocument();
    expect(document.querySelector("main input[type='file']")).toBeNull();
    // The only file input is the toolbar's image picker, deliberately outside
    // the page surface; there is no second, cover-shaped picker.
    expect(document.querySelectorAll("input[type='file']")).toHaveLength(1);
    expect(screen.getByTestId("page-toolbar-image-input")).toBeInTheDocument();
  });

  it("shows no merge-conflict dialog and no shortcut reference", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByText(/keyboard shortcuts?/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /shortcuts?/i })).not.toBeInTheDocument();
  });

  it("keeps the options menu free of lock, move and new-tab entries", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Page options" }));
    const labels = (await screen.findAllByRole("menuitem")).map((item) => item.textContent ?? "");
    for (const absent of ["Lock", "Unlock", "Open in new tab", "Move to project…"]) {
      expect(labels).not.toContain(absent);
    }
  });
});

describe("PageDetailScreen mention and entity peeks", () => {
  it("publishes the workspace member directory for mention chips (PAGE-041)", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    await waitFor(() => expect(mentionNameFor(PAGES_USER_ID, "Stored")).toBe("Ada Byron"));
  });

  it("opens the shell's issue peek from an entity request and closes it (PAGE-043)", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    requestEntityPeek({
      entityName: "issue",
      entityIdentifier: "ISS-2",
      projectIdentifier: "WEB",
      workspaceIdentifier: "acme",
      projectId: PROJECT,
      label: "Fix login",
    });
    expect(await screen.findByRole("button", { name: "Close issue peek" })).toBeInTheDocument();
    expect(issuePeek).toHaveBeenCalledWith(
      expect.objectContaining({ peeked: "ISS-2", peekProjectId: PROJECT, projectIdentifier: "WEB" })
    );
    fireEvent.click(screen.getByRole("button", { name: "Close issue peek" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Close issue peek" })).not.toBeInTheDocument());
  });
});

describe("PageDetailScreen header actions (PAGE-055-060)", () => {
  async function openOptions(): Promise<void> {
    fireEvent.click(screen.getByRole("button", { name: "Page options" }));
    await screen.findByRole("menuitem", { name: "Version history" });
  }

  it("duplicates the page from the options menu", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Make a copy" }));
    await waitFor(() => expect(harness.pageWrites.some((write) => write.method === "DUPLICATE")).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith("Page duplicated successfully.");
  });

  it("shelves and restores the page from the menu with the marker following", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    await waitFor(() => expect(harness.pageWrites.some((write) => write.method === "ARCHIVE")).toBe(true));
    expect(await screen.findByTestId("page-archived-marker")).toHaveTextContent(/^Archived at .+, 2026$/);
    expect(notifySuccess).toHaveBeenCalledWith("Page archived successfully.");
    // The shelved page reads back read-only with no visibility entries.
    await openOptions();
    expect(await screen.findByRole("menuitem", { name: "Restore" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Make private" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));
    await waitFor(() => expect(harness.pageWrites.some((write) => write.method === "RESTORE")).toBe(true));
    await waitFor(() => expect(screen.queryByTestId("page-archived-marker")).not.toBeInTheDocument());
    expect(notifySuccess).toHaveBeenCalledWith("Page restored successfully.");
  });

  it("switches visibility both ways from the menu", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Make private" }));
    await waitFor(() => expect(harness.pageWrites).toContainEqual({ method: "ACCESS", body: { access: 1 } }));
    await openOptions();
    fireEvent.click(await screen.findByRole("menuitem", { name: "Make public" }));
    await waitFor(() => expect(harness.pageWrites).toContainEqual({ method: "ACCESS", body: { access: 0 } }));
  });

  it("toggles the lock from the header control", async () => {
    renderScreen();
    await screen.findByRole("textbox", { name: "Page title" });
    fireEvent.click(screen.getByRole("button", { name: "Lock" }));
    await waitFor(() => expect(harness.pageWrites.some((write) => write.method === "LOCK")).toBe(true));
    fireEvent.click(await screen.findByRole("button", { name: "Locked" }));
    await waitFor(() => expect(harness.pageWrites.some((write) => write.method === "UNLOCK")).toBe(true));
  });

  it("deletes a shelved page after confirmation and lands on the shelved tab", async () => {
    harness.page = record({ archived_at: "2026-10-20" });
    renderScreen();
    expect(await screen.findByTestId("page-archived-marker")).toBeInTheDocument();
    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete page" });
    expect(within(dialog).getByText("Launch notes")).toBeInTheDocument();
    // A shelved page does not carry its subtree count, so the dialog reads the
    // children once and holds Delete until it knows how many are re-homed.
    await waitFor(() => expect(screen.queryByTestId("page-delete-rehomed-loading")).not.toBeInTheDocument());
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(harness.pageWrites.some((write) => write.method === "DELETE")).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith("Page deleted successfully.");
    await waitFor(() =>
      expect(router.navigate).toHaveBeenCalledWith({
        href: `/acme/projects/${PROJECT}/pages?type=archived`,
      })
    );
  });

  it("leaves a shelved page untouched when the delete is cancelled", async () => {
    harness.page = record({ archived_at: "2026-10-20" });
    renderScreen();
    expect(await screen.findByTestId("page-archived-marker")).toBeInTheDocument();
    await openOptions();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete page" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Delete page" })).not.toBeInTheDocument());
    expect(harness.pageWrites.some((write) => write.method === "DELETE")).toBe(false);
    expect(router.navigate).not.toHaveBeenCalled();
    expect(screen.getByTestId("page-archived-marker")).toBeInTheDocument();
  });
});
