// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  isRedirect,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RouterContext } from "../../../__root.js";
import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommand,
  findPalettePage,
  listCommands,
} from "../../../../shared/commands/index.js";
import { sessionKeys, setSessionClient } from "../../../../core/session/index.js";
import { ASYNC_QUERY_TIMEOUT_MS } from "../../../../testing/testBudget.js";
import { projectIdentifier, Route } from "./$issueKey.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const UUID = "123e4567-e89b-12d3-a456-426614174000";
// The strip appears after the router's loader and two query waves. Under a
// full run those share workers with the Tiptap chunk, so it uses the same
// load allowance as the per-test budget. The strip itself is only ever late,
// never absent, in the cases that expect it.
const STRIP_TIMEOUT = { timeout: ASYNC_QUERY_TIMEOUT_MS };

function findStrip(): Promise<HTMLElement> {
  return screen.findByTestId("project-tab-strip", undefined, STRIP_TIMEOUT);
}

const CHILD_ID = "523e4567-e89b-12d3-a456-426614174005";
const OTHER_PROJECT_ID = "623e4567-e89b-12d3-a456-426614174006";

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

function liteRow(overrides: Record<string, unknown> = {}) {
  return {
    id: ISSUE_ID,
    sequence_id: 1,
    name: "Fix login",
    description_html: "<p></p>",
    sort_order: 1,
    project_id: PROJECT_ID,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    is_epic: false,
    is_intake: false,
    is_synced: false,
    archived_at: null,
    ...overrides,
  };
}

function issueRow() {
  return {
    id: ISSUE_ID,
    name: "Fix login",
    state_id: UUID,
    sort_order: 1.5,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 1,
    project_id: PROJECT_ID,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: null,
    link_count: 0,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
  };
}

function detailsRow(overrides: Record<string, unknown> = {}) {
  return {
    ...projectRow(),
    description: "",
    description_text: null,
    description_html: null,
    default_assignee: null,
    emoji: null,
    icon_prop: null,
    intake_view: true,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: UUID,
    archive_in: 0,
    close_in: 0,
    default_state: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    deleted_at: null,
    ...overrides,
  };
}

function meRow() {
  return {
    id: UUID,
    email: "ada@example.com",
    username: "ada",
    first_name: "Ada",
    last_name: "L",
    display_name: "Ada L",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: "2024-01-02T03:04:05Z",
    user_timezone: "UTC",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "",
    last_login_time: null,
  };
}

const STATE_ROW = {
  id: "423e4567-e89b-12d3-a456-426614174003",
  project_id: PROJECT_ID,
  workspace_id: UUID,
  name: "Done",
  color: "#00ff00",
  group: "completed",
  default: false,
  description: "",
  sequence: 2,
};

function userPropertiesPayload() {
  return {
    id: UUID,
    workspace: UUID,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: "TABBED",
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

interface BrowseExtras {
  details?: "member" | "archived";
  me?: boolean;
  unresolvedWorkspace?: boolean;
  archivedIssue?: boolean;
  syncedIssue?: boolean;
  estimate?: string | null;
  intake?: boolean;
  liteStatus?: number;
  published?: boolean;
  child?: boolean;
  foreignChild?: boolean;
  creator?: boolean;
  memberRole?: number;
}

function renderBrowse(known: boolean, isEpic?: boolean, extras: BrowseExtras = {}) {
  const calls: string[] = [];
  const apiClient = createAppClient("https://api.test", async (url: string, init: { method: string }) => {
    calls.push(`${init.method} ${url}`);
    let status = 404;
    let body = "{}";
    if (url.endsWith("/auth/get-csrf-token/")) {
      status = 200;
      body = JSON.stringify({ csrf_token: "csrf" });
    } else if (init.method !== "GET" && url.includes(`/issues/${ISSUE_ID}/`)) {
      status = 204;
      body = "";
    } else if (url.includes("/work-items/WEB-1/")) {
      status = extras.liteStatus ?? (known ? 200 : 404);
      if (status === 200) {
        const lite = liteRow({
          ...(isEpic === undefined ? {} : { is_epic: isEpic }),
          ...(extras.intake === true ? { is_intake: true } : {}),
          ...(extras.archivedIssue === true ? { archived_at: "2024-03-04" } : {}),
        });
        body = JSON.stringify(lite);
      }
    } else if (url.endsWith("/users/me/workspaces/")) {
      status = 200;
      body = JSON.stringify([{ id: UUID, name: "Acme", slug: "acme", logo_url: null, total_members: 4, role: 20 }]);
    } else if (url.includes("/project-roles/")) {
      status = 200;
      body = JSON.stringify({ [PROJECT_ID]: 20 });
    } else if (url.includes(`/projects/${PROJECT_ID}/user-properties/`)) {
      status = 200;
      body = JSON.stringify({
        ...userPropertiesPayload(),
        project: PROJECT_ID,
        preferences: { navigation: { default_tab: "work_items", hide_in_more_menu: [] } },
        sort_order: 1,
      });
    } else if (url.endsWith("/user-properties/")) {
      status = 200;
      body = JSON.stringify(userPropertiesPayload());
    } else if (url.endsWith("/projects/")) {
      status = 200;
      body = JSON.stringify([
        projectRow(),
        ...(extras.foreignChild === true
          ? [{ ...projectRow(), id: OTHER_PROJECT_ID, name: "Betel", identifier: "BET", is_default: false }]
          : []),
      ]);
    } else if (url.includes("/history/")) {
      if (url.includes("activity_type=issue-property")) {
        body = JSON.stringify([]);
        status = 200;
      } else {
        status = 200;
        body = JSON.stringify([
          {
            id: "423e4567-e89b-12d3-a456-426614174003",
            issue: ISSUE_ID,
            project: PROJECT_ID,
            workspace: UUID,
            actor: UUID,
            actor_detail: {
              id: UUID,
              first_name: "Ada",
              last_name: "L",
              avatar: "",
              avatar_url: null,
              display_name: "Ada L",
              is_bot: false,
            },
            comment_html: "<p>Detail thread</p>",
            comment_stripped: "Detail thread",
            comment_json: {},
            attachments: [],
            labels: [],
            access: "INTERNAL",
            edited_at: null,
            comment_reactions: [],
            speaker_type: "human",
            speaker_label: "",
            speaker_agent_run_id: null,
            parent: null,
            external_source: null,
            external_id: null,
            is_synced: false,
            description: null,
            created_at: "2024-02-03T04:05:06Z",
            updated_at: "2024-02-03T04:05:06Z",
            created_by: UUID,
            updated_by: null,
            deleted_at: null,
          },
        ]);
      }
    } else if (url.includes("/reactions/")) {
      status = 200;
      body = JSON.stringify([]);
    } else if (url.includes("/sub-issues/")) {
      status = 200;
      body = JSON.stringify({
        sub_issues:
          extras.child === true || extras.foreignChild === true
            ? [
                {
                  ...issueRow(),
                  id: CHILD_ID,
                  name: "Every inspector",
                  sequence_id: 2,
                  parent_id: ISSUE_ID,
                  ...(extras.foreignChild === true ? { project_id: OTHER_PROJECT_ID, sequence_id: 9 } : {}),
                  state_group: "completed",
                },
              ]
            : [],
        state_distribution: extras.child === true || extras.foreignChild === true ? { completed: [CHILD_ID] } : {},
        total_count: extras.child === true || extras.foreignChild === true ? 1 : 0,
      });
    } else if (url.includes("/issue-relation/")) {
      status = 200;
      body = JSON.stringify({
        blocking: [],
        blocked_by: [],
        duplicate: [],
        relates_to: [],
        start_after: [],
        start_before: [],
        finish_after: [],
        finish_before: [],
      });
    } else if (url.includes("/issue-links/")) {
      status = 200;
      body = JSON.stringify([]);
    } else if (url.endsWith(`/issues/${ISSUE_ID}/attachments/`)) {
      status = 200;
      body = JSON.stringify([]);
    } else if (url.endsWith(`/issues/${ISSUE_ID}/code-reviews/`)) {
      status = 200;
      body = JSON.stringify([]);
    } else if (url.includes("/api/instances/")) {
      status = 200;
      body = JSON.stringify({
        config: {
          enable_signup: true,
          is_workspace_creation_disabled: false,
          is_google_enabled: false,
          is_github_enabled: false,
          is_gitlab_enabled: false,
          is_gitea_enabled: false,
          is_magic_login_enabled: true,
          is_email_password_enabled: true,
          is_smtp_configured: false,
          is_self_managed: true,
          instance_changelog_url: "",
        },
      });
    } else if (url.includes(`/issues/${ISSUE_ID}/`)) {
      status = 200;
      const row = {
        ...(isEpic === undefined ? issueRow() : { ...issueRow(), is_epic: isEpic }),
        ...(extras.creator === true ? { created_by: UUID } : {}),
      };
      const stamped = extras.archivedIssue === true ? { ...row, archived_at: "2024-03-04" } : row;
      body = JSON.stringify(extras.syncedIssue === true ? { ...stamped, is_synced: true } : stamped);
    } else if (url.endsWith("/projects/details/")) {
      status = 200;
      if (extras.details !== undefined) {
        const overrides: Record<string, unknown> =
          extras.details === "archived" ? { archived_at: "2024-03-04T05:06:07Z" } : {};
        if (extras.estimate !== undefined) overrides["estimate"] = extras.estimate;
        if (extras.published === true) overrides["anchor"] = "board";
        if (extras.memberRole !== undefined) overrides["member_role"] = extras.memberRole;
        body = JSON.stringify([detailsRow(overrides)]);
      } else {
        body = "[]";
      }
    } else if (url.endsWith("/api/users/me/")) {
      if (extras.me === true) {
        status = 200;
        body = JSON.stringify(meRow());
      }
    } else if (url.includes("/states/")) {
      status = 200;
      body = JSON.stringify([STATE_ROW]);
    } else if (url.includes("description-versions")) {
      status = 200;
      body = JSON.stringify({
        prev_cursor: "",
        cursor: "",
        next_cursor: null,
        prev_page_results: false,
        next_page_results: false,
        page_count: 0,
        total_results: 0,
        total_pages: 0,
        results: [],
      });
    } else if (url.endsWith("/members/")) {
      status = 200;
      body = JSON.stringify([]);
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  setSessionClient(apiClient);
  if (extras.unresolvedWorkspace !== true) {
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: UUID, name: "Acme", slug: "acme", logo_url: null, total_members: 4, role: 20 },
    ]);
  }
  const component = Route.options.component;
  if (!component) throw new Error("missing component");
  const rootRoute = createRootRouteWithContext<RouterContext>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <Outlet />,
  });
  const gatedRoute = createRoute({
    getParentRoute: () => wsRoute,
    id: "_gated",
    component: () => <Outlet />,
  });
  const browseRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "browse/$issueKey",
    component,
  });
  const awayRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "$",
    component: () => <p>navigated away</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([gatedRoute.addChildren([browseRoute]), awayRoute])]),
    context: { queryClient, apiClient },
    history: createMemoryHistory({ initialEntries: ["/acme/browse/WEB-1"] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router, calls };
}

afterEach(() => {
  resetAppClient();
});

describe("browse route", () => {
  it("renders the detail under the project strip with tracking highlighted", async () => {
    renderBrowse(true);
    await findStrip();
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
    // No project details resolve, so the title reads as a heading.
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(document.title).toBe("WEB-1 Fix login");
  });

  it("mounts attachments in the widget root and hides code reviews at zero", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    expect(await within(widgets).findByRole("region", { name: "Attachments" })).toBeInTheDocument();
    expect(within(widgets).getAllByRole("button", { name: "Attach" }).length).toBeGreaterThan(0);
    expect(within(widgets).queryByText("Code reviews")).not.toBeInTheDocument();
    expect(widgets.textContent ?? "").not.toMatch(/work\s?log/i);
  });

  it("routes the action row's Attach entry to the attachment picker", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    const actions = await within(widgets).findAllByRole("button", { name: "Attach" });
    const actionRow = actions.find((node) => node.getAttribute("data-widget-action") === "Attach");
    expect(actionRow).toBeDefined();
    await waitFor(() => expect(actionRow as HTMLElement).toBeEnabled());
    const clicked = vi.spyOn(HTMLInputElement.prototype, "click");
    await userEvent.setup().click(actionRow as HTMLElement);
    expect(clicked).toHaveBeenCalled();
    clicked.mockRestore();
  });

  it("keeps the widget root's sections in their fixed order", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await within(widgets).findByRole("region", { name: "Attachments" });
    const labels = [...widgets.querySelectorAll("[data-widget-section]")].map((node) =>
      node.getAttribute("data-widget-section")
    );
    expect(labels).toEqual(["Relations", "Links", "Attachments"]);
  });

  it("mounts run-AI in the widget action row and re-tick on the detail", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    const runAI = within(widgets).getByTestId("issue-run-ai");
    expect(runAI).toHaveTextContent("Manually Run AI");
    await waitFor(() => expect(runAI).toBeEnabled());
    expect(screen.queryByTestId("issue-re-tick")).not.toBeInTheDocument();
  });

  it("keeps run-AI inert until the workspace read resolves", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, unresolvedWorkspace: true });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    expect(within(widgets).getByTestId("issue-run-ai")).toBeDisabled();
  });

  it("edits the title inline once membership resolves", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const box = await screen.findByRole("textbox", { name: "Title" });
    expect(box).toHaveValue("Fix login");
  });

  it("mounts the comment thread under the description with the composer", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    expect(await screen.findByRole("heading", { name: "Activity" })).toBeInTheDocument();
    expect(await screen.findByText("Detail thread")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Add comment" })).toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Comment visibility" })).not.toBeInTheDocument();
  });

  it("keeps the issue reaction row on the description-meta row above the activity block", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const reactions = await screen.findByRole("region", { name: "Reactions on this work item" });
    const meta = reactions.closest('[data-slot="description-meta"]');
    expect(meta).not.toBeNull();
    const activity = await screen.findByRole("heading", { name: "Activity" });
    expect(meta).not.toContainElement(activity);
    expect(meta?.compareDocumentPosition(activity) ?? 0).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(meta).toContainElement(await screen.findByRole("button", { name: /Edited by/ }));
  });

  it("shows the comment access switch on a published project", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, published: true });
    expect(await screen.findByRole("radiogroup", { name: "Comment visibility" })).toBeInTheDocument();
  });

  it("drops the comment composer on an archived issue but keeps its cards", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, archivedIssue: true });
    expect(await screen.findByText("Detail thread")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Add comment" })).not.toBeInTheDocument();
  });

  it("shows the history line read-only for archived issues", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, archivedIssue: true });
    // Membership resolves (the line renders), yet the title stays a heading.
    await screen.findByRole("button", { name: /Edited by/ });
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Title" })).not.toBeInTheDocument();
  });

  it("mounts the widget stack with live relation and link entries for a member", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    const widget = await screen.findByTestId("issue-detail-widgets");
    await waitFor(() => expect(within(widget).getByRole("button", { name: "Add relation" })).toBeEnabled());
    expect(within(widget).getByRole("button", { name: "Add link" })).toBeEnabled();
  });

  it("keeps the widget stack read-only on an archived issue", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, archivedIssue: true });
    const widget = await screen.findByTestId("issue-detail-widgets");
    expect(within(widget).getByRole("button", { name: "Add relation" })).toBeDisabled();
    expect(within(widget).getByRole("button", { name: "Add link" })).toBeDisabled();
  });

  it("keeps the widget stack read-only on a synced issue", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, syncedIssue: true });
    const widget = await screen.findByTestId("issue-detail-widgets");
    expect(within(widget).getByRole("button", { name: "Add relation" })).toBeDisabled();
    expect(within(widget).getByRole("button", { name: "Add link" })).toBeDisabled();
  });

  it("renders unknown keys without the strip, linking home", async () => {
    renderBrowse(false);
    await screen.findByRole("heading", { name: "No work item matches this address" });
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to the workspace home" })).toHaveAttribute("href", "/acme");
    // Missing keys claim no detail title; the shell fallback wins.
    expect(document.title).not.toContain("WEB-1");
  });

  it("hides a refused item behind the missing state with no composer (CMT-002)", async () => {
    // A guest refused another member's item resolves the same null lite
    // as an unknown key (issues §21 owns the read); the comments area
    // only verifies the state carries no posting surface.
    renderBrowse(true, undefined, { liteStatus: 403 });
    await screen.findByRole("heading", { name: "No work item matches this address" });
    expect(screen.queryByRole("group", { name: "Add comment" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Comment" })).not.toBeInTheDocument();
  });

  it("leaves intake rows for the project intake tab", async () => {
    const { router } = renderBrowse(true, undefined, { intake: true });
    expect(await screen.findByText("navigated away")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe(`/acme/projects/${PROJECT_ID}/intake`);
    expect(router.state.location.search).toMatchObject({ currentTab: "open", inboxIssueId: ISSUE_ID });
  });

  it("highlights nothing for an epic while no epic tab renders", async () => {
    renderBrowse(true, true);
    await findStrip();
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Work Items" })).not.toHaveAttribute("aria-current");
    expect(document.querySelectorAll('[aria-current="page"]')).toHaveLength(0);
  });

  it("registers the work-item contextual group for the resolved issue", async () => {
    renderBrowse(true);
    await findStrip();
    // The stub answers no project details, so only the copy entries show.
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.copy-id`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Copy ID");
    expect(titles).toContain("Copy title");
    expect(titles).toContain("Copy URL");
    expect(titles).not.toContain("Change state");
    clearCommands();
    clearPalettePages();
  });

  it("offers the editing entries once the project details resolve", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    await findStrip();
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Change state");
    expect(titles).toContain("Change priority");
    expect(titles).toContain("Assign to");
    expect(titles).toContain("Assign to me");
    expect(titles).toContain("Change estimate");
    expect(titles).toContain("Add to cycle");
    expect(titles).toContain("Add to modules");
    expect(titles).toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("hides only the estimate picker when the project sets no estimate system", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, estimate: null });
    await findStrip();
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Change state");
    expect(titles).not.toContain("Change estimate");
    clearCommands();
    clearPalettePages();
  });

  it("hides editing entries for archived issues", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, archivedIssue: true });
    await findStrip();
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.copy-id`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Copy ID");
    expect(titles).toContain("Copy title");
    expect(titles).toContain("Copy URL");
    expect(titles).not.toContain("Change state");
    expect(titles).not.toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("hides editing entries for archived projects", async () => {
    renderBrowse(true, undefined, { details: "archived", me: true });
    await findStrip();
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.copy-id`)).toBeDefined());
    const titles = listCommands().map((command) => command.title);
    expect(titles).toContain("Copy ID");
    expect(titles).not.toContain("Change state");
    expect(titles).not.toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("lets a plain member delete the work item they created (ISS-222)", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, creator: true, memberRole: 15 });
    await findStrip();
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    expect(listCommands().map((command) => command.title)).toContain("Delete");
    clearCommands();
    clearPalettePages();
  });

  it("mounts the work-item actions beside the detail header (ISS-066)", async () => {
    renderBrowse(true, undefined, { details: "member", me: true });
    await findStrip();
    expect(await screen.findByRole("button", { name: "Work item actions" })).toBeInTheDocument();
  });

  it("leaves the header menu off a synced work item", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, syncedIssue: true });
    await screen.findByTestId("issue-detail-widgets");
    expect(screen.queryByRole("button", { name: "Work item actions" })).toBeNull();
  });

  it("refreshes the detail after a state pick", async () => {
    const { calls } = renderBrowse(true, undefined, { details: "member", me: true });
    await findStrip();
    await waitFor(() => expect(findPalettePage(`context.work-item.${ISSUE_ID}.state`)).toBeDefined());
    await waitFor(() =>
      expect(findPalettePage(`context.work-item.${ISSUE_ID}.state`)!.usePageState("").options).toHaveLength(1)
    );
    await act(async () => {
      findPalettePage(`context.work-item.${ISSUE_ID}.state`)!.usePageState("").options[0]!.run();
    });
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("PATCH") && call.includes(`/issues/${ISSUE_ID}/`))).toBe(true)
    );
    clearCommands();
    clearPalettePages();
  });

  it("leaves for the issue list after a confirmed delete", async () => {
    const user = userEvent.setup();
    const { router } = renderBrowse(true, undefined, { details: "member", me: true });
    await findStrip();
    await waitFor(() => expect(findCommand(`context.work-item.${ISSUE_ID}.delete`)).toBeDefined());
    await act(async () => {
      await findCommand(`context.work-item.${ISSUE_ID}.delete`)!.run();
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText("navigated away")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe(`/acme/projects/${PROJECT_ID}/issues`);
    clearCommands();
    clearPalettePages();
  });
});

describe("browse widgets", () => {
  it("falls back to the key's prefix until the project row arrives", () => {
    expect(projectIdentifier(undefined, "WEB-12")).toBe("WEB");
    expect(projectIdentifier({ identifier: "ATL" }, "WEB-12")).toBe("ATL");
    expect(projectIdentifier(undefined, "NOKEY")).toBe("");
  });

  it("mounts the widget under the description with the project locked", async () => {
    renderBrowse(true, undefined, { details: "member", me: true, child: true });
    await findStrip();
    expect(await screen.findByText("Every inspector")).toBeInTheDocument();
    // The detail sidebar also carries a sub-work-item action; the widget's
    // own row is the one inside the widgets root.
    const widget = screen.getByTestId("issue-detail-widgets");
    const add = within(widget).getByRole("button", { name: "Add sub-work item" });
    expect(add).toBeEnabled();
    const user = userEvent.setup();
    await user.click(add);
    await user.click(await screen.findByRole("menuitem", { name: "Create new" }));
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Web" })).toBeDisabled();
    expect(screen.getByTestId("parent-tag")).toHaveTextContent("Fix login");
  });

  it("opens a child row at its own browse key", async () => {
    const { router } = renderBrowse(true, undefined, { details: "member", me: true, child: true });
    await findStrip();
    await userEvent.setup().click(await screen.findByRole("link", { name: /Every inspector/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/acme/browse/WEB-2"));
  });

  it("opens a child from another project under that project's key", async () => {
    const { router } = renderBrowse(true, undefined, { details: "member", me: true, foreignChild: true });
    await findStrip();
    const link = await screen.findByRole("link", { name: /Every inspector/ });
    expect(link).toHaveAttribute("href", "/acme/browse/BET-9");
    await userEvent.setup().click(link);
    await waitFor(() => expect(router.state.location.pathname).toBe("/acme/browse/BET-9"));
  });

  it("keeps the add entry shut for a guest", async () => {
    renderBrowse(true, undefined, { child: true });
    await findStrip();
    expect(await screen.findByText("Every inspector")).toBeInTheDocument();
    const widget = await screen.findByTestId("issue-detail-widgets");
    expect(within(widget).getByRole("button", { name: "Add sub-work item" })).toBeDisabled();
  });
});

describe("browse loader", () => {
  function loaderClient(stub: { liteStatus?: number; intake?: boolean }) {
    const calls: string[] = [];
    const apiClient = createAppClient("https://api.test", async (url: string, init: { method: string }) => {
      calls.push(`${init.method} ${url}`);
      let status = 404;
      let body = "{}";
      if (url.includes("/work-items/WEB-1/")) {
        status = stub.liteStatus ?? 200;
        if (status === 200) {
          body = JSON.stringify(liteRow(stub.intake === true ? { is_intake: true } : {}));
        }
      } else if (url.includes(`/issues/${ISSUE_ID}/`)) {
        status = 200;
        body = JSON.stringify(issueRow());
      }
      return { status, url, headers: { get: () => null }, text: async () => body };
    });
    return { apiClient, queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }), calls };
  }

  type LoaderInput = { context: { queryClient: QueryClient; apiClient: unknown }; params: unknown };

  function getLoader(): (input: LoaderInput) => Promise<unknown> {
    const loader = Route.options.loader as unknown as (input: LoaderInput) => Promise<unknown>;
    if (typeof loader !== "function") throw new Error("missing loader");
    return loader;
  }

  async function runLoader(stub: { liteStatus?: number; intake?: boolean }): Promise<unknown> {
    const { apiClient, queryClient } = loaderClient(stub);
    return getLoader()({ context: { queryClient, apiClient }, params: { ws: "acme", issueKey: "WEB-1" } });
  }

  it("prefetches the detail behind a resolved key", async () => {
    const { apiClient, queryClient, calls } = loaderClient({});
    await getLoader()({ context: { queryClient, apiClient }, params: { ws: "acme", issueKey: "WEB-1" } });
    expect(calls.some((call) => call.includes("/work-items/WEB-1/"))).toBe(true);
    expect(calls.some((call) => call.includes(`/issues/${ISSUE_ID}/`))).toBe(true);
  });

  it("redirects intake rows without hydrating", async () => {
    const thrown = await runLoader({ intake: true }).then(
      () => null,
      (error: unknown) => error
    );
    expect(thrown).not.toBeNull();
    expect(isRedirect(thrown)).toBe(true);
    const options = (thrown as unknown as { options?: { to?: unknown; params?: unknown; search?: unknown } }).options;
    expect(options?.to).toBe("/$ws/projects/$projectId/intake");
    expect(options?.params).toEqual({ ws: "acme", projectId: PROJECT_ID });
    expect(options?.search).toEqual({ currentTab: "open", inboxIssueId: ISSUE_ID });
  });

  it("hydrates nothing for unknown keys", async () => {
    const { apiClient, queryClient, calls } = loaderClient({ liteStatus: 404 });
    await getLoader()({ context: { queryClient, apiClient }, params: { ws: "acme", issueKey: "WEB-1" } });
    expect(calls).toHaveLength(1);
  });

  it("rejects when the lite fetch fails", async () => {
    await expect(runLoader({ liteStatus: 500 })).rejects.toThrow();
  });
});

describe("browse chrome", () => {
  it("shows the skeleton while pending and retries from errors", async () => {
    const user = userEvent.setup();
    const Pending = Route.options.pendingComponent as unknown as React.ComponentType;
    const { unmount } = render(<Pending />);
    expect(screen.getByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    unmount();
    const ErrorView = Route.options.errorComponent as unknown as React.ComponentType<{
      error: unknown;
      reset: () => void;
    }>;
    let resets = 0;
    const { rerender } = render(<ErrorView error={new Error("Downstream is down")} reset={() => (resets += 1)} />);
    expect(screen.getByRole("heading", { name: "Could not load this work item" })).toBeInTheDocument();
    expect(screen.getByText("Downstream is down")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(resets).toBe(1);
    rerender(<ErrorView error="string failure" reset={() => {}} />);
    expect(screen.getByText("Something went wrong.")).toBeInTheDocument();
  });
});
