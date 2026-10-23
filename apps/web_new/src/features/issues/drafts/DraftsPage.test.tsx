// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace drafts screen (parity/specs/issues.md §12, ISS-137–141): the
// first read, pagination, every row flow, and the two first-run screens.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { DraftIssue, DraftIssueListResponse, ProjectDetails } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toasts = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));
const ids = vi.hoisted(() => ({
  ME: "423e4567-e89b-42d3-8456-426614174003",
  SOMEONE: "523e4567-e89b-42d3-9456-426614174004",
}));
vi.mock("../../../shared/shell/index.js", () => toasts);

const session = vi.hoisted(() => ({
  me: { data: { id: ids.ME } as { id: string } | undefined },
  canEdit: true,
  isAdmin: true,
  role: 20,
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: () => session.me,
  usePermissions: () => ({
    role: session.role,
    isMember: session.role >= 5,
    canEdit: session.canEdit,
    isAdmin: session.isAdmin,
  }),
}));

import { identifierOf, DraftsPage } from "./DraftsPage.js";
import type { DraftDialogProps } from "./DraftsPage.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const DRAFT_1 = "123e4567-e89b-12d3-a456-426614174000";
const DRAFT_2 = "323e4567-e89b-12d3-a456-426614174002";

function draft(id: string, name: string, overrides: Partial<DraftIssue> = {}): DraftIssue {
  return {
    id,
    name,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: ids.ME,
    updated_by: null,
    type_id: null,
    description_html: "",
    ...overrides,
  } as DraftIssue;
}

function page(results: DraftIssue[], overrides: Partial<DraftIssueListResponse> = {}): DraftIssueListResponse {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: "",
    prev_cursor: "",
    next_page_results: false,
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
    ...overrides,
  };
}

function project(id = PROJECT, name = "Atlas", overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "623e4567-e89b-42d3-8456-426614174005",
    name,
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "PAR",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: 0,
    member_role: 20,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "",
    ...overrides,
  } as unknown as ProjectDetails;
}

interface StubOptions {
  pages?: DraftIssueListResponse[];
  pagesAfterCreate?: DraftIssueListResponse[];
  projects?: ProjectDetails[];
  listStatus?: number;
  patchStatus?: number;
  patchGate?: Promise<void>;
  deleteStatus?: number;
  deleteBody?: unknown;
  promoteStatus?: number;
  createStatus?: number;
  projectsStatus?: number | "pending";
}

function stubClient(options: StubOptions = {}) {
  const calls: Array<{ url: string; method: string; body: unknown }> = [];
  let created = false;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string } = {}) => {
    const method = init.method ?? "GET";
    const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
    calls.push({ url, method, body });
    const json = (status: number, payload: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(payload),
    });
    if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.includes(`/api/workspaces/${SLUG}/draft-issues/`) && method === "GET") {
      if (options.listStatus !== undefined) return json(options.listStatus, { error: "boom" });
      const rows = created ? (options.pagesAfterCreate ?? options.pages) : options.pages;
      return json(200, rows?.[0] ?? page([]));
    }
    if (
      url.includes(`/api/workspaces/${SLUG}/draft-issues/`) &&
      !/draft-issues\/[^/?]+/.test(url) &&
      method === "POST"
    ) {
      created = true;
      return json(options.createStatus ?? 201, draft(DRAFT_2, "New draft"));
    }
    if (url.includes(`/api/workspaces/${SLUG}/projects/details/`)) {
      if (options.projectsStatus === "pending") return new Promise(() => undefined);
      if (options.projectsStatus !== undefined) return json(options.projectsStatus, { error: "boom" });
      return json(200, options.projects ?? [project()]);
    }
    if (url.includes(`/draft-issues/`) && method === "DELETE") {
      return json(options.deleteStatus ?? 204, options.deleteBody ?? {});
    }
    if (/draft-issues\/[^/?]+\/$/.test(url) && method === "PATCH") {
      if (options.patchGate !== undefined) await options.patchGate;
      if (options.patchStatus !== undefined && options.patchStatus >= 400) {
        return json(options.patchStatus, { error: "boom" });
      }
      return json(200, draft(DRAFT_1, "One", { priority: "high" }));
    }
    if (url.includes("/draft-to-issue/") && method === "POST") {
      return json(options.promoteStatus ?? 200, {
        id: DRAFT_1,
        name: "One",
        project_id: PROJECT,
        sequence_id: 5,
        state_id: null,
        priority: "none",
        sort_order: 1,
        parent_id: null,
        description_html: "",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        deleted_at: null,
      });
    }
    return json(200, {});
  }) as never);
  return { calls };
}

/** A dialog stand-in that reports what the page handed it. */
function dialogStub() {
  const seen: DraftDialogProps[] = [];
  function CreateDialog(props: DraftDialogProps): React.ReactElement {
    seen.push(props);
    return <div data-testid="create-dialog" data-move={String(props.moveToProject ?? false)} />;
  }
  return { CreateDialog, seen };
}

function renderPage(
  options: StubOptions = {},
  handlers: { onCreateProject?: () => void; seedProjects?: boolean } = {}
) {
  const stub = stubClient(options);
  const dialog = dialogStub();
  const onCreateProject = handlers.onCreateProject ?? vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  if (handlers.seedProjects !== false) {
    queryClient.setQueryData(projectKeys.details(SLUG), options.projects ?? [project()]);
  }
  const result = render(
    <QueryClientProvider client={queryClient}>
      <DraftsPage workspaceSlug={SLUG} CreateDialog={dialog.CreateDialog} onCreateProject={onCreateProject} />
    </QueryClientProvider>
  );
  void result;
  return { ...result, ...stub, dialog, onCreateProject, queryClient };
}

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/api/keys.js";

beforeEach(() => {
  toasts.notifySuccess.mockReset();
  toasts.notifyError.mockReset();
  session.me = { data: { id: ids.ME } };
  session.canEdit = true;
  session.isAdmin = true;
  session.role = 20;
});

afterEach(() => {
  resetAppClient();
});

function openMenu(name = "One") {
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${name}` }));
  return within(screen.getByRole("menu"));
}

describe("project lookups", () => {
  it("answers nothing for a draft without a project", () => {
    expect(identifierOf([project()], null)).toBeNull();
  });

  it("answers nothing for a project the viewer cannot list", () => {
    expect(identifierOf([], PROJECT)).toBeNull();
  });

  it("reads the project's code", () => {
    expect(identifierOf([project(PROJECT, "Atlas")], PROJECT)).toBe("PAR");
  });
});

describe("first paint", () => {
  it("shows a skeleton while the list resolves", () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    expect(screen.getByTestId("drafts-loading")).toBeInTheDocument();
  });

  it("lists each draft with the count and the create entry", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")], { total_count: 2 })] });
    expect(await screen.findByText("One")).toBeInTheDocument();
    expect(screen.getByText("Two")).toBeInTheDocument();
    expect(screen.getByTestId("drafts-count")).toHaveTextContent("2");
    expect(screen.getByRole("button", { name: "Draft a work item" })).toBeEnabled();
  });

  it("hides the count chip and the header entry without drafts or projects", async () => {
    renderPage({ pages: [page([])], projects: [] });
    expect(await screen.findByText("No project")).toBeInTheDocument();
    expect(screen.queryByTestId("drafts-count")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Draft a work item" })).not.toBeInTheDocument();
  });

  it("explains a failed read and retries", async () => {
    renderPage({ listStatus: 500 });
    expect(await screen.findByText("Couldn't load drafts")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Try again" })).toBeEnabled();
  });

  it("disables the create entry for a guest", async () => {
    session.canEdit = false;
    renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    expect(screen.getByRole("button", { name: "Draft a work item" })).toBeDisabled();
  });
});

describe("pagination", () => {
  it("loads the next page from the load-more row", async () => {
    let served = 0;
    createAppClient("https://api.test", (async (url: string) => {
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/")) {
        served += 1;
        return served === 1
          ? json(200, page([draft(DRAFT_1, "One")], { total_count: 2, next_page_results: true, next_cursor: "c2" }))
          : json(200, page([draft(DRAFT_2, "Two")], { total_count: 2 }));
      }
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialogStub().CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    fireEvent.click(screen.getByRole("button", { name: /Load more/ }));
    expect(await screen.findByText("Two")).toBeInTheDocument();
  });

  it("shows a loader row while the next page is in flight", async () => {
    let release: (() => void) | null = null;
    const blocked = new Promise<void>((resolve) => {
      release = resolve;
    });
    let served = 0;
    createAppClient("https://api.test", (async (url: string) => {
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/")) {
        served += 1;
        if (served === 1) {
          return json(
            200,
            page([draft(DRAFT_1, "One")], { total_count: 2, next_page_results: true, next_cursor: "c2" })
          );
        }
        await blocked;
        return json(200, page([draft(DRAFT_2, "Two")], { total_count: 2 }));
      }
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialogStub().CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    // The observer starts on the load-more row: its click is the request.
    act(() => {
      fireEvent.click(screen.getByRole("button", { name: /Load more/ }));
    });
    expect(await screen.findByTestId("drafts-load-more-loading")).toBeInTheDocument();
    act(() => release?.());
    expect(await screen.findByText("Two")).toBeInTheDocument();
    expect(screen.queryByTestId("drafts-load-more-loading")).not.toBeInTheDocument();
  });
});

describe("first-run screens", () => {
  it("invites the first draft and opens the create dialog", async () => {
    const h = renderPage({ pages: [page([])] });
    expect(await screen.findByText("Half-written work items")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create draft work item" }));
    expect(screen.getByTestId("create-dialog")).toBeInTheDocument();
    expect(h.dialog.seen.at(-1)).toMatchObject({ open: true });
  });

  it("disables the first-draft action for a guest", async () => {
    session.canEdit = false;
    renderPage({ pages: [page([])] });
    expect(await screen.findByRole("button", { name: "Create draft work item" })).toBeDisabled();
  });

  it("waits for the project list before deciding the viewer has none", async () => {
    // Drafts can land first; a project list still in flight must not flash
    // the "no project" screen over a list the viewer may well have.
    renderPage({ pages: [page([draft(DRAFT_1, "One")])], projectsStatus: "pending" }, { seedProjects: false });
    expect(await screen.findByTestId("drafts-loading")).toBeInTheDocument();
    expect(screen.queryByText("No project")).not.toBeInTheDocument();
    expect(screen.queryByText("One")).not.toBeInTheDocument();
  });

  it("refuses a draft the viewer has no identity for", async () => {
    session.me = { data: undefined };
    session.canEdit = false;
    session.isAdmin = false;
    session.role = 5;
    renderPage({
      pages: [page([draft(DRAFT_1, "One")])],
      projects: [project(PROJECT, "Atlas", { member_role: 15 })],
    });
    expect(await screen.findByText("One")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Actions for One" })).not.toBeInTheDocument();
  });

  it("leaves the code off a draft that holds no project", async () => {
    const h = renderPage({
      pages: [page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two", { project_id: null })])],
    });
    expect(await screen.findByText("One")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy PAR" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Copy / })).toHaveLength(1);
    expect(h.dialog.seen).toBeDefined();
  });

  it("treats an archived joined project as no project", async () => {
    renderPage({
      pages: [page([])],
      projects: [project(PROJECT, "Atlas", { archived_at: "2026-01-01T00:00:00Z" })],
    });
    expect(await screen.findByText("No project")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Draft a work item" })).not.toBeInTheDocument();
  });

  it("asks a viewer without a project to create one", async () => {
    const h = renderPage({ pages: [page([])], projects: [project(PROJECT, "Atlas", { member_role: null })] });
    expect(await screen.findByText("No project")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(h.onCreateProject).toHaveBeenCalledTimes(1);
  });
});

describe("row flows", () => {
  it("opens edit through the menu with the draft's snapshot", async () => {
    const h = renderPage({
      pages: [
        page([
          draft(DRAFT_1, "One", {
            description_html: "<p>body</p>",
            state_id: "723e4567-e89b-12d3-a456-426614174006",
            priority: "high",
            label_ids: ["813e4567-e89b-42d3-8456-426614174009"],
            assignee_ids: ["913e4567-e89b-42d3-8456-426614174010"],
            start_date: "2026-02-01",
            target_date: "2026-02-10",
          }),
        ]),
      ],
    });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Edit" }));
    const props = h.dialog.seen.at(-1);
    expect(props?.edit).toMatchObject({ kind: "draft", id: DRAFT_1, projectId: PROJECT });
    expect(props?.edit?.snapshot).toMatchObject({
      name: "One",
      description_html: "<p>body</p>",
      state_id: "723e4567-e89b-12d3-a456-426614174006",
      priority: "high",
      label_ids: ["813e4567-e89b-42d3-8456-426614174009"],
      assignee_ids: ["913e4567-e89b-42d3-8456-426614174010"],
      start_date: "2026-02-01",
      target_date: "2026-02-10",
    });
  });

  it("opens a copy with the duplicated payload", async () => {
    const h = renderPage({
      pages: [
        page([
          draft(DRAFT_1, "One", {
            label_ids: ["813e4567-e89b-42d3-8456-426614174009"],
            description_html: "<p>Carried body</p>",
            state_id: "713e4567-e89b-42d3-8456-426614174007",
            priority: "high",
          }),
        ]),
      ],
    });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Make a copy" }));
    const props = h.dialog.seen.at(-1);
    expect(props?.draftSeed).toEqual({
      title: "One (copy)",
      input: expect.objectContaining({
        name: "One (copy)",
        project_id: PROJECT,
        description_html: "<p>Carried body</p>",
        state_id: "713e4567-e89b-42d3-8456-426614174007",
        priority: "high",
        label_ids: ["813e4567-e89b-42d3-8456-426614174009"],
      }),
    });
    expect(props?.onCopySaved).toBeTypeOf("function");
  });

  it("saves a copy through the create write", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Make a copy" }));
    await h.dialog.seen.at(-1)?.onCopySaved?.({ name: "One (copy)", project_id: PROJECT });
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "POST" && call.url.endsWith("/draft-issues/"))).toBe(true)
    );
    expect(toasts.notifySuccess).toHaveBeenCalledWith("Success!", "Draft created");
  });

  it("opens the move dialog in move mode and closes it after a promote", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Move to project" }));
    const props = h.dialog.seen.at(-1);
    expect(props).toMatchObject({ moveToProject: true });
    await props?.onPublish?.({ name: "One", project_id: PROJECT });
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument());
    expect(toasts.notifySuccess).toHaveBeenCalledWith("Success", "Draft published to project.");
  });

  it("re-reads the list after the dialog reports a saved edit", async () => {
    let reads = 0;
    createAppClient("https://api.test", (async (url: string) => {
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/") && !/draft-issues\/[^/?]+/.test(url)) {
        reads += 1;
        return json(200, page([draft(DRAFT_1, reads === 1 ? "One" : "One edited")]));
      }
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const dialog = dialogStub();
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialog.CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Edit" }));
    await act(async () => {
      dialog.seen.at(-1)?.edit?.onChanged?.();
    });
    expect(await screen.findByText("One edited")).toBeInTheDocument();
    expect(reads).toBeGreaterThan(1);
  });

  it("edits on double-click and closes on the dialog's request", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByRole("article", { name: "Draft One" });
    fireEvent.doubleClick(screen.getByRole("article", { name: "Draft One" }));
    const props = h.dialog.seen.at(-1);
    expect(props?.edit).toMatchObject({ kind: "draft", id: DRAFT_1 });
    props?.onOpenChange(false);
    await waitFor(() => expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument());
  });

  it("gives each block the properties strip bound to its own row", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")])] });
    await screen.findByText("One");
    const blocks = screen.getAllByRole("article");
    expect(blocks).toHaveLength(2);
    for (const block of blocks) expect(within(block).getByTestId("draft-properties")).toBeInTheDocument();
  });
});

describe("delete", () => {
  it("asks first, then deletes and toasts", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu("One").getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete draft" })).toHaveTextContent("One");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "Draft One" })).not.toBeInTheDocument());
    expect(h.calls.some((call) => call.method === "DELETE" && call.url.includes(DRAFT_1))).toBe(true);
    expect(toasts.notifySuccess).toHaveBeenCalledWith("Success!", "Draft deleted");
  });

  it("does not write when the pending draft is cleared as the dialog closes", async () => {
    // While the delete write is in flight the dialog already reports closed,
    // so a second confirm — or a late settle — must not resend or throw.
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Delete draft" })).not.toBeInTheDocument());
    expect(h.calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
  });

  it("cancels without writing", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Delete draft" })).not.toBeInTheDocument();
    expect(h.calls.some((call) => call.method === "DELETE")).toBe(false);
  });

  // DRAFT-025 hides row actions the viewer may not use, so a
  // non-creator never reaches the UI delete path; the server's refusal
  // is the observable behavior (duplicate-delete.spec.ts, DRAFT-011).
  it("refuses a member who did not create the draft", async () => {
    session.isAdmin = false;
    session.role = 15;
    renderPage({
      pages: [page([draft(DRAFT_1, "One", { created_by: ids.SOMEONE })])],
      projects: [project(PROJECT, "Atlas", { member_role: 15 })],
    });
    await screen.findByText("One");
    expect(screen.queryByRole("button", { name: "Actions for One" })).not.toBeInTheDocument();
  });

  it("lets a workspace admin delete someone else's draft from any project standing", async () => {
    renderPage({
      pages: [page([draft(DRAFT_1, "One", { created_by: ids.SOMEONE })])],
      projects: [project(PROJECT, "Atlas", { member_role: 15 })],
    });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete draft" })).toBeInTheDocument();
  });

  it("refuses a project-only admin who is not a workspace admin", async () => {
    session.isAdmin = false;
    session.role = 15;
    renderPage({
      pages: [page([draft(DRAFT_1, "One", { created_by: ids.SOMEONE })])],
      projects: [project(PROJECT, "Atlas", { member_role: 20 })],
    });
    await screen.findByText("One");
    expect(screen.queryByRole("button", { name: "Actions for One" })).not.toBeInTheDocument();
  });

  it("lets a non-admin creator delete their own draft", async () => {
    session.isAdmin = false;
    session.role = 15;
    renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete draft" })).toBeInTheDocument();
  });

  it("lets a guest edit and delete a draft they created", async () => {
    session.canEdit = false;
    session.isAdmin = false;
    session.role = 5;
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    expect(screen.getByRole("button", { name: "Draft a work item" })).toBeDisabled();
    const menu = openMenu();
    expect(menu.queryByRole("menuitem", { name: "Make a copy" })).not.toBeInTheDocument();
    expect(menu.queryByRole("menuitem", { name: "Move to project" })).not.toBeInTheDocument();
    fireEvent.click(menu.getByRole("menuitem", { name: "Edit" }));
    expect(h.dialog.seen.at(-1)?.edit).toMatchObject({ kind: "draft", id: DRAFT_1 });
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete draft" })).toBeInTheDocument();
  });

  it("passes the guest a dialog that cannot publish", async () => {
    session.canEdit = false;
    session.isAdmin = false;
    session.role = 5;
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Edit" }));
    expect(h.dialog.seen.at(-1)).toMatchObject({ canPublish: false });
  });

  it("maps an inline PATCH refusal to the permission notice and rolls back", async () => {
    createAppClient("https://api.test", (async (url: string, init: { method?: string } = {}) => {
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/") && (init.method ?? "GET") === "GET") {
        return json(200, page([draft(DRAFT_1, "One")]));
      }
      if (/draft-issues\/[^/?]+\/$/.test(url) && init.method === "PATCH") return json(403, { error: "Forbidden" });
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialogStub().CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    fireEvent.click(within(await screen.findByTestId("draft-properties")).getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));
    await waitFor(() =>
      expect(toasts.notifyError).toHaveBeenCalledWith(
        "You don't have permission to perform this action",
        "Only a workspace admin or this draft's creator can change it."
      )
    );
  });

  it("maps a promote refusal to the permission notice and keeps the dialog", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])], promoteStatus: 403 });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Move to project" }));
    const props = h.dialog.seen.at(-1);
    await act(async () => {
      await props?.onPublish?.({ name: "One", project_id: PROJECT }).catch(() => undefined);
    });
    expect(toasts.notifyError).toHaveBeenCalledWith(
      "You don't have permission to perform this action",
      "Only a workspace member or admin can publish a draft."
    );
    expect(screen.getByTestId("create-dialog")).toBeInTheDocument();
  });

  it("explains a generic promote failure and keeps the dialog", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])], promoteStatus: 500 });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Move to project" }));
    const props = h.dialog.seen.at(-1);
    await act(async () => {
      await props?.onPublish?.({ name: "One", project_id: PROJECT }).catch(() => undefined);
    });
    expect(toasts.notifyError).toHaveBeenCalledWith("Error!", "Failed to move work item to project. Please try again.");
    expect(screen.getByTestId("create-dialog")).toBeInTheDocument();
  });

  it("reads a server permission refusal distinctly", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One")])], deleteStatus: 403, deleteBody: { error: "Forbidden" } });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(toasts.notifyError).toHaveBeenCalledWith(
        "You don't have permission to perform this action",
        expect.any(String)
      )
    );
  });

  it("reads a generic server failure as a generic one", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One")])], deleteStatus: 500, deleteBody: { error: "boom" } });
    await screen.findByText("One");
    fireEvent.click(openMenu().getByRole("menuitem", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(toasts.notifyError).toHaveBeenCalledWith("Couldn't delete the work item", expect.any(String))
    );
  });
});

describe("cache", () => {
  it("sends an inline property pick as a patch to that draft", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(within(await screen.findByTestId("draft-properties")).getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));
    await waitFor(() =>
      expect(
        h.calls.find((call) => call.method === "PATCH" && call.url.includes(`/draft-issues/${DRAFT_1}/`))
      ).toMatchObject({ body: { priority: "high" } })
    );
  });

  it("sends one row's picks in order, holding the second until the first lands", async () => {
    let releaseFirst!: () => void;
    const firstGate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const sent: unknown[] = [];
    createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string } = {}) => {
      const method = init.method ?? "GET";
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/") && !/draft-issues\/[^/?]+/.test(url)) {
        return json(200, page([draft(DRAFT_1, "One")]));
      }
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      if (/draft-issues\/[^/?]+\/$/.test(url) && method === "PATCH") {
        const body: unknown = init.body === undefined ? undefined : JSON.parse(init.body);
        if (sent.length === 0) await firstGate;
        sent.push(body);
        return json(200, { ...(body as object) });
      }
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialogStub().CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    const strip = await screen.findByTestId("draft-properties");
    fireEvent.click(within(strip).getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));
    // The first pick paints at once while its PATCH waits on the gate.
    expect(await within(strip).findByRole("button", { name: "High" })).toBeInTheDocument();
    fireEvent.click(within(strip).getByRole("button", { name: "High" }));
    fireEvent.click(await screen.findByRole("option", { name: "Low" }));
    // The second PATCH holds until the first one answers: no second
    // write and no second paint while the gate stays shut.
    await act(async () => {
      await Promise.resolve();
    });
    expect(sent).toEqual([]);
    expect(within(strip).queryByRole("button", { name: "Low" })).not.toBeInTheDocument();
    await act(async () => {
      releaseFirst();
    });
    await waitFor(() => expect(sent).toEqual([{ priority: "high" }, { priority: "low" }]));
    expect(await within(strip).findByRole("button", { name: "Low" })).toBeInTheDocument();
  });

  it("rolls a rejected pick back on the row and explains the failure", async () => {
    // The gate holds the failing PATCH so the optimistic paint stays
    // observable, mirroring the oracle's delayed failure.
    let release!: () => void;
    const patchGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderPage({ pages: [page([draft(DRAFT_1, "One")])], patchStatus: 500, patchGate });
    await screen.findByText("One");
    const strip = await screen.findByTestId("draft-properties");
    fireEvent.click(within(strip).getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "High" }));
    expect(await within(strip).findByRole("button", { name: "High" })).toBeInTheDocument();
    await act(async () => {
      release();
    });
    expect(await within(strip).findByRole("button", { name: "None" })).toBeInTheDocument();
    await waitFor(() =>
      expect(toasts.notifyError).toHaveBeenCalledWith("Couldn't save the change", expect.any(String))
    );
  });
});

describe("header and count (DRAFT-001/002/003)", () => {
  it("renders rows in server order under the Drafts heading", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "Second"), draft(DRAFT_2, "First")])] });
    expect(await screen.findByText("Second")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Drafts" })).toBeInTheDocument();
    const blocks = screen.getAllByRole("article");
    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toHaveTextContent("Second");
    expect(blocks[1]).toHaveTextContent("First");
  });

  it("shows the server total in the chip, not the loaded rows", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One")], { total_count: 51 })] });
    await screen.findByText("One");
    expect(screen.getByTestId("drafts-count")).toHaveTextContent("51");
  });

  it("opens the create dialog from the header entry", async () => {
    const h = renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    fireEvent.click(screen.getByRole("button", { name: "Draft a work item" }));
    expect(screen.getByTestId("create-dialog")).toBeInTheDocument();
    expect(h.dialog.seen.at(-1)).toMatchObject({ open: true });
  });
});

describe("paging polish (DRAFT-023)", () => {
  it("drops a repeated row and hides Load more at the end", async () => {
    let served = 0;
    createAppClient("https://api.test", (async (url: string) => {
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/")) {
        served += 1;
        return served === 1
          ? json(200, page([draft(DRAFT_1, "One")], { total_count: 2, next_page_results: true, next_cursor: "c2" }))
          : json(200, page([draft(DRAFT_1, "One"), draft(DRAFT_2, "Two")], { total_count: 2 }));
      }
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialogStub().CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    fireEvent.click(screen.getByRole("button", { name: /Load more/ }));
    await screen.findByText("Two");
    expect(screen.getAllByRole("article")).toHaveLength(2);
    expect(screen.queryByRole("button", { name: /Load more/ })).not.toBeInTheDocument();
    expect(screen.getByTestId("drafts-count")).toHaveTextContent("2");
  });

  it("keeps its rows and retries when the next page fails", async () => {
    let release: (() => void) | null = null;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    let served = 0;
    createAppClient("https://api.test", (async (url: string) => {
      const json = (status: number, payload: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(payload),
      });
      if (url.includes("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      if (url.includes("/draft-issues/")) {
        served += 1;
        if (served === 1) {
          return json(
            200,
            page([draft(DRAFT_1, "One")], { total_count: 2, next_page_results: true, next_cursor: "c2" })
          );
        }
        if (served === 2) {
          // Held until the loader is observed, then refused once.
          await held;
          return json(500, { error: "boom" });
        }
        return json(200, page([draft(DRAFT_2, "Two")], { total_count: 2 }));
      }
      if (url.endsWith("/projects/details/")) return json(200, [project()]);
      return json(200, {});
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <DraftsPage workspaceSlug={SLUG} CreateDialog={dialogStub().CreateDialog} onCreateProject={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("One");
    fireEvent.click(screen.getByRole("button", { name: /Load more/ }));
    expect(await screen.findByTestId("drafts-load-more-loading")).toBeInTheDocument();
    act(() => release?.());
    // The failure keeps the loaded rows and hands the control back for a retry.
    await waitFor(() => expect(screen.getByRole("button", { name: /Load more/ })).toBeInTheDocument());
    expect(screen.getByText("One")).toBeInTheDocument();
    expect(screen.queryByText("Two")).not.toBeInTheDocument();
    expect(screen.queryByText("Couldn't load drafts")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Load more/ }));
    expect(await screen.findByText("Two")).toBeInTheDocument();
  });
});

describe("project read states (DRAFT-022/023)", () => {
  it("waits for the project list before inviting the first draft", async () => {
    // Empty drafts with projects still in flight: neither first-run
    // screen may flash in while the viewer may well have a project.
    const h = renderPage({ pages: [page([])], projectsStatus: "pending" }, { seedProjects: false });
    await waitFor(() => expect(h.calls.some((call) => call.url.includes("/draft-issues/"))).toBe(true));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 25));
    });
    expect(screen.getByTestId("drafts-loading")).toBeInTheDocument();
    expect(screen.queryByText("No project")).not.toBeInTheDocument();
    expect(screen.queryByText("Half-written work items")).not.toBeInTheDocument();
  });

  it("explains a failed project read and retries both reads", async () => {
    const h = renderPage({ pages: [page([])], projectsStatus: 500 }, { seedProjects: false });
    expect(await screen.findByText("Couldn't load drafts")).toBeInTheDocument();
    const projectReads = () => h.calls.filter((call) => call.url.includes("/projects/details/")).length;
    expect(projectReads()).toBe(1);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(projectReads()).toBe(2));
  });

  it("keeps counted rows under the no-project guidance", async () => {
    // NEWFRONT-305: a counted draft always renders. With rows but no
    // usable project, the guidance sits above the list, not over it.
    renderPage({ pages: [page([draft(DRAFT_1, "One")], { total_count: 1 })], projects: [] });
    expect(await screen.findByText("No project")).toBeInTheDocument();
    expect(screen.getByText("One")).toBeInTheDocument();
    expect(screen.getByTestId("drafts-list")).toBeInTheDocument();
    expect(screen.getByTestId("drafts-count")).toHaveTextContent("1");
    expect(screen.queryByRole("button", { name: "Draft a work item" })).not.toBeInTheDocument();
    const guidance = screen.getByTestId("drafts-no-project-state");
    const position = guidance.compareDocumentPosition(screen.getByTestId("drafts-list"));
    expect(position & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("disables the create-project action for a guest", async () => {
    session.canEdit = false;
    renderPage({ pages: [page([])], projects: [] });
    expect(await screen.findByRole("button", { name: "Start your first project" })).toBeDisabled();
  });
});

describe("non-behaviors (DRAFT-026)", () => {
  it("offers no checkbox, draggable row, filter, search or export", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    for (const block of screen.getAllByRole("article")) {
      expect(block).not.toHaveAttribute("draggable");
    }
    const text = document.body.textContent ?? "";
    expect(text).not.toMatch(/filter/i);
    expect(text).not.toMatch(/export/i);
    expect(text).not.toMatch(/search/i);
  });

  it("ignores shortcut-like keys", async () => {
    renderPage({ pages: [page([draft(DRAFT_1, "One")])] });
    await screen.findByText("One");
    for (const key of ["e", "/", "?", "c"]) {
      fireEvent.keyDown(document.body, { key });
    }
    expect(screen.queryByTestId("create-dialog")).not.toBeInTheDocument();
    expect(screen.getByText("One")).toBeInTheDocument();
  });
});
