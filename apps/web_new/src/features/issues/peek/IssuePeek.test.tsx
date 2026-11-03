// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys, setSessionClient } from "../../../core/session/index.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { issueKeys } from "../api/keys.js";
import { ResolvingIssuePeek } from "./ResolvingIssuePeek.js";
import {
  IssuePeek,
  keepPeekOnClick,
  NESTED_PEEK_TEST_ID,
  PEEK_MISSING_BODY,
  PEEK_MISSING_TITLE,
  peekIdentifierOf,
  peekKeyOf,
  peekLinkKeyOf,
} from "./IssuePeek.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const CHILD = "523e4567-e89b-12d3-a456-426614174005";
const UUID = "123e4567-e89b-12d3-a456-426614174000";
const ADA = "823e4567-e89b-12d3-a456-426614174007";
const STATE = "423e4567-e89b-12d3-a456-426614174003";
const ESTIMATE_PROJECT_SYSTEM = "c13e4567-e89b-12d3-a456-42661417400f";
const ESTIMATE_POINT_ONE = "c23e4567-e89b-12d3-a456-426614174010";
const ESTIMATE_POINT_TWO = "c33e4567-e89b-12d3-a456-426614174011";

function projectRow() {
  return {
    id: PROJECT,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
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
    estimate: null,
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
    next_work_item_sequence: 2,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    deleted_at: null,
    ...overrides,
  };
}

function issueRow(overrides: Record<string, unknown> = {}) {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: STATE,
    sort_order: 1.5,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    start_date: null,
    target_date: null,
    sequence_id: 1,
    project_id: PROJECT,
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
    description_html: "<p>Current words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

const STATE_ROW = {
  id: STATE,
  project_id: PROJECT,
  workspace_id: UUID,
  name: "Done",
  color: "#00ff00",
  group: "completed",
  default: false,
  description: "",
  sequence: 2,
};

const DIRECTORY = [
  {
    id: "b23e4567-e89b-12d3-a456-42661417400a",
    member: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: null,
      display_name: "Ada L",
      is_bot: false,
    },
    role: 20,
    is_active: true,
  },
];

const PROJECT_MEMBERS = [
  {
    id: "d23e4567-e89b-12d3-a456-42661417400c",
    role: 20,
    member: ADA,
    project: PROJECT,
    original_role: 20,
    created_at: "2024-01-02T03:04:05Z",
  },
];

function meRow() {
  return {
    id: ADA,
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

const WORKSPACES = [{ id: UUID, name: "Acme", slug: SLUG, logo_url: null, total_members: 2, role: 20 }];

const EMPTY_VERSIONS = {
  prev_cursor: "",
  cursor: "",
  next_cursor: null,
  prev_page_results: false,
  next_page_results: false,
  page_count: 0,
  total_results: 0,
  total_pages: 0,
  results: [],
};

interface PeekExtras {
  /** Defers the detail read until the returned release runs. */
  holdDetail?: boolean;
  /** Defers the nested child read until the returned release runs. */
  holdChild?: boolean;
  /** Detail answers with this status instead of the row. */
  detailStatus?: number;
  /** Child detail answers with this status instead of the row. */
  childStatus?: number;
  /** PATCH answers with this status. */
  patchStatus?: number;
  /** Project details answer (absent leaves the project unknown). */
  details?: Record<string, unknown> | "error";
  /** Single project read answers with this status. */
  projectStatus?: number;
  /** Include the child row in the sub-issues page. */
  child?: boolean;
  /** Creator, member role, epic, archived, synced, published overrides. */
  creator?: boolean;
  epic?: boolean;
  archivedIssue?: boolean;
  syncedIssue?: boolean;
  published?: boolean;
  /** Skip seeding the session workspace (unresolved workspace). */
  unresolvedWorkspace?: boolean;
  /** Render an origin row outside the panel for the refocus test. */
  withOrigin?: boolean;
  /** Route project code fallback (defaults to WEB). */
  projectIdentifier?: string;
  /** Peeked key (defaults to WEB-1; null simulates a UUID deep link). */
  issueKey?: string | null;
  /** Seed the detail cache empty (defensive empty-resolution path). */
  emptyDetail?: boolean;
  /** The estimate the peeked issue carries (EST-022). */
  estimatePoint?: string | null;
  /** Answer the project estimate read with one points system. */
  estimates?: boolean;
  /** Mount with the peek closed (the refocus test opens it after focusing). */
  startClosed?: boolean;
  /** Mount embedded (Escape and outside-click close nothing). */
  embedded?: boolean;
  /** Hand nested changes to a host that owns the address (ISS-167). */
  onNestedChange?: (selection: { issueId: string; projectId: string; nestingLevel: number }) => void;
  /** Mount the resolving peek behind a host that writes peek params (ISS-167). */
  resolve?: boolean;
}

interface PeekHarness {
  onClose: ReturnType<typeof vi.fn>;
  writes: Array<{ method: string; url: string; body: unknown }>;
  releaseDetail: () => void;
  releaseChild: () => void;
  setOpen: (open: boolean) => void;
  unmount: () => void;
  queryClient: QueryClient;
  router: { state: { location: { pathname: string; search: unknown } } };
}

/**
 * Host that owns the peek address in state, the way a route writer does
 * through the search params (ISS-167).
 */
function ResolvingHost({ onClose }: { onClose: () => void }): React.ReactElement {
  const [address, setAddress] = React.useState<{ peeked: string; projectId: string | null }>({
    peeked: ISSUE,
    projectId: null,
  });
  return (
    <>
      <button type="button" onClick={() => setAddress({ peeked: CHILD, projectId: null })}>
        Move address to child
      </button>
      <ResolvingIssuePeek
        workspaceSlug={SLUG}
        projectId={PROJECT}
        projectIdentifier="WEB"
        peeked={address.peeked}
        peekProjectId={address.projectId}
        rows={[{ id: ISSUE, project_id: PROJECT, sequence_id: 1 }]}
        onPeekChange={(selection) =>
          setAddress({
            peeked: selection.issueId,
            projectId: selection.projectId === PROJECT ? null : selection.projectId,
          })
        }
        onClose={onClose}
      />
    </>
  );
}

function renderPeek(extras: PeekExtras = {}): PeekHarness {
  const onClose = vi.fn();
  let subscribed = false;
  const writes: Array<{ method: string; url: string; body: unknown }> = [];
  let releaseDetail: () => void = () => {};
  const detailGate = extras.holdDetail === true ? new Promise<void>((resolve) => (releaseDetail = resolve)) : null;
  let releaseChild: () => void = () => {};
  const childGate = extras.holdChild === true ? new Promise<void>((resolve) => (releaseChild = resolve)) : null;
  const apiClient = createAppClient("https://api.test", (async (
    url: string,
    init: { method: string; body?: string }
  ) => {
    if (init.method !== "GET") writes.push({ method: init.method, url, body: init.body ?? "" });
    let status = 404;
    let body = "{}";
    const answer = (
      code: number,
      payload: string
    ): { status: number; url: string; headers: { get: () => null }; text: () => Promise<string> } => ({
      status: code,
      url,
      headers: { get: () => null },
      text: async () => payload,
    });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return answer(200, JSON.stringify({ csrf_token: "csrf" }));
    }
    if (init.method === "PATCH" && url.includes("/issues/")) {
      const code = extras.patchStatus ?? 204;
      return answer(code, code === 204 ? "" : JSON.stringify({ error: "nope" }));
    }
    if (init.method === "POST" && url.endsWith("/subscribe/")) {
      subscribed = true;
      return answer(200, JSON.stringify({ id: UUID, subscriber: ADA }));
    }
    if (init.method === "DELETE" && url.endsWith("/subscribe/")) {
      subscribed = false;
      return answer(200, JSON.stringify({ subscribed: false }));
    }
    if (init.method === "POST" && url.endsWith("/archive/")) {
      return answer(200, JSON.stringify({ archived_at: "2026-03-01" }));
    }
    if (init.method === "DELETE" && url.endsWith("/archive/")) {
      return answer(204, "");
    }
    if (init.method === "DELETE" && url.includes("/issues/")) {
      return answer(204, "");
    }
    if (url.includes("/sub-issues/")) {
      if (url.includes(`/issues/${CHILD}/`) || extras.child !== true) {
        return answer(200, JSON.stringify({ sub_issues: [], state_distribution: {}, total_count: 0 }));
      }
      return answer(
        200,
        JSON.stringify({
          sub_issues: [
            {
              ...issueRow({ id: CHILD, name: "Every inspector", sequence_id: 2, parent_id: ISSUE }),
              state_group: "completed",
            },
          ],
          state_distribution: { completed: [CHILD] },
          total_count: 1,
        })
      );
    }
    if (url.includes("/issue-relation/")) {
      return answer(
        200,
        JSON.stringify({
          blocking: [],
          blocked_by: [],
          duplicate: [],
          relates_to: [],
          start_after: [],
          start_before: [],
          finish_after: [],
          finish_before: [],
        })
      );
    }
    if (url.includes("/issue-links/") || url.includes("/attachments/") || url.includes("/code-reviews/")) {
      return answer(200, "[]");
    }
    if (url.includes("/history/") || url.includes("/reactions/")) {
      return answer(200, "[]");
    }
    if (url.includes("description-versions")) {
      return answer(200, JSON.stringify(EMPTY_VERSIONS));
    }
    if (url.includes("/states/")) {
      return answer(200, JSON.stringify([STATE_ROW]));
    }
    if (url.includes(`/projects/${PROJECT}/estimates/`)) {
      if (extras.estimates !== true) return answer(200, "[]");
      return answer(
        200,
        JSON.stringify([
          {
            id: ESTIMATE_PROJECT_SYSTEM,
            workspace: UUID,
            project: PROJECT,
            name: "Points",
            description: "",
            type: "points",
            last_used: true,
            points: [
              { id: ESTIMATE_POINT_ONE, key: 1, value: "1", description: "" },
              { id: ESTIMATE_POINT_TWO, key: 2, value: "2", description: "" },
            ],
            created_at: "2024-01-02T03:04:05Z",
            updated_at: "2024-02-03T04:05:06Z",
            created_by: null,
            updated_by: null,
          },
        ])
      );
    }
    if (url.includes(`/projects/${PROJECT}/members/`)) {
      return answer(200, JSON.stringify(PROJECT_MEMBERS));
    }
    if (url.includes("/members/")) {
      return answer(200, JSON.stringify(DIRECTORY));
    }
    if (url.includes("/api/instances/")) {
      return answer(
        200,
        JSON.stringify({
          config: {
            enable_signup: false,
            is_workspace_creation_disabled: true,
            is_google_enabled: false,
            is_github_enabled: false,
            is_gitlab_enabled: false,
            is_gitea_enabled: false,
            is_magic_login_enabled: false,
            is_email_password_enabled: false,
            is_smtp_configured: false,
            is_self_managed: true,
            instance_changelog_url: "",
          },
          instance: {
            id: "00000000-0000-0000-0000-000000000001",
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            created_by: null,
            updated_by: null,
            deleted_at: null,
            instance_name: "Test",
            whitelist_emails: null,
            instance_id: "test-instance",
            current_version: "9.9.9",
            latest_version: null,
            edition: "oss",
            domain: "example.test",
            last_checked_at: "2026-01-01T00:00:00Z",
            namespace: null,
            is_telemetry_enabled: false,
            is_support_required: false,
            is_setup_done: true,
            is_signup_screen_visited: true,
            is_verified: false,
            is_test: true,
            is_current_version_deprecated: false,
            workspaces_exist: true,
          },
        })
      );
    }
    if (url.endsWith("/subscribe/")) {
      return answer(200, JSON.stringify({ subscribed }));
    }
    if (url.includes(`/issues/${CHILD}/`)) {
      if (childGate !== null) await childGate;
      const code = extras.childStatus ?? 200;
      if (code !== 200) return answer(code, "{}");
      return answer(
        200,
        JSON.stringify(issueRow({ id: CHILD, name: "Every inspector", sequence_id: 2, parent_id: ISSUE }))
      );
    }
    if (url.includes(`/issues/${ISSUE}/`)) {
      if (detailGate !== null) await detailGate;
      const code = extras.detailStatus ?? 200;
      if (code !== 200) return answer(code, "{}");
      return answer(
        200,
        JSON.stringify(
          issueRow({
            ...(extras.creator === true ? { created_by: ADA } : {}),
            ...(extras.epic === true ? { is_epic: true } : {}),
            ...(extras.archivedIssue === true ? { archived_at: "2024-03-04" } : {}),
            ...(extras.syncedIssue === true ? { is_synced: true } : {}),
            ...(extras.estimatePoint === undefined ? {} : { estimate_point: extras.estimatePoint }),
          })
        )
      );
    }
    if (url.endsWith("/projects/details/")) {
      if (extras.details === undefined) return answer(200, "[]");
      if (extras.details === "error") return answer(500, "{}");
      return answer(
        200,
        JSON.stringify([detailsRow({ ...(extras.published === true ? { anchor: "board" } : {}), ...extras.details })])
      );
    }
    if (url.endsWith(`/projects/${PROJECT}/`)) {
      const code = extras.projectStatus ?? 200;
      if (code !== 200) return answer(code, "{}");
      return answer(
        200,
        JSON.stringify(detailsRow({ estimate: extras.estimates === true ? ESTIMATE_PROJECT_SYSTEM : null }))
      );
    }
    if (url.endsWith("/projects/")) {
      return answer(200, JSON.stringify([projectRow()]));
    }
    return answer(status, body);
  }) as never);
  setSessionClient(apiClient);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(["session", "me"], meRow());
  if (extras.unresolvedWorkspace !== true) {
    queryClient.setQueryData(sessionKeys.workspaces, WORKSPACES);
  }
  if (extras.emptyDetail === true) {
    queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), undefined);
  }
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  let open = extras.startClosed !== true;
  const listeners = new Set<() => void>();
  const setOpen = (next: boolean): void => {
    open = next;
    listeners.forEach((notify) => notify());
  };
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const issuesRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/issues",
    component: function PeekHost() {
      const [, force] = React.useReducer((count: number) => count + 1, 0);
      React.useEffect(() => {
        listeners.add(force);
        return () => {
          listeners.delete(force);
        };
      }, []);
      return (
        <>
          {extras.withOrigin === true ? <button type="button">Origin row</button> : null}
          {open ? (
            extras.resolve === true ? (
              <ResolvingHost onClose={onClose} />
            ) : (
              <IssuePeek
                workspaceSlug={SLUG}
                projectId={PROJECT}
                projectIdentifier={extras.projectIdentifier ?? "WEB"}
                issueId={ISSUE}
                issueKey={extras.issueKey === undefined ? "WEB-1" : extras.issueKey}
                embedded={extras.embedded}
                {...(extras.onNestedChange === undefined ? {} : { onNestedChange: extras.onNestedChange })}
                onClose={onClose}
              />
            )
          ) : null}
        </>
      );
    },
  });
  const browseRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "browse/$issueKey",
    component: () => <p>full page</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([issuesRoute, browseRoute])]),
    history: createMemoryHistory({ initialEntries: [`/${SLUG}/projects/${PROJECT}/issues?peekIssueId=WEB-1`] }),
  });
  const mounted = render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { onClose, writes, releaseDetail, releaseChild, setOpen, queryClient, router, unmount: mounted.unmount };
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("peekKeyOf", () => {
  it("keeps the opener key with the loaded project code", () => {
    expect(peekKeyOf("WEB-1", "WEB", "WEB", 1)).toEqual({ key: "WEB-1", identifier: "WEB" });
  });

  it("falls back to the key prefix without a loaded project", () => {
    expect(peekKeyOf("WEB-1", undefined, "WEB", 1)).toEqual({ key: "WEB-1", identifier: "WEB" });
  });

  it("derives a deep-link key from the loaded project", () => {
    expect(peekKeyOf(null, "WEB", "ZZZ", 1)).toEqual({ key: "WEB-1", identifier: "WEB" });
  });

  it("derives a deep-link key from the route fallback", () => {
    expect(peekKeyOf(null, undefined, "ZZZ", 1)).toEqual({ key: "ZZZ-1", identifier: "ZZZ" });
  });
});

describe("peekLinkKeyOf", () => {
  it("links through the opener key once the opener knew it", () => {
    expect(peekLinkKeyOf("WEB-1", undefined, 1)).toBe("WEB-1");
  });

  it("derives the key from a loaded project for id links", () => {
    expect(peekLinkKeyOf(null, "WEB", 7)).toBe("WEB-7");
  });

  it("stays unresolved without a key or a loaded project", () => {
    expect(peekLinkKeyOf(null, undefined, 7)).toBeNull();
  });
});

describe("peekIdentifierOf", () => {
  it("prefers the loaded project, then the key prefix, then the fallback", () => {
    expect(peekIdentifierOf("WEB-1", "WEB", "ZZZ")).toBe("WEB");
    expect(peekIdentifierOf("WEB-1", undefined, "ZZZ")).toBe("WEB");
    expect(peekIdentifierOf(null, undefined, "ZZZ")).toBe("ZZZ");
  });
});

describe("keepPeekOnClick", () => {
  it.each([{ metaKey: true }, { ctrlKey: true }, { shiftKey: true }, { altKey: true }, { button: 1 }])(
    "keeps the peek up for %s",
    (override) => {
      expect(
        keepPeekOnClick({ metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, button: 0, ...override })
      ).toBe(true);
    }
  );

  it("lets a plain primary click through", () => {
    expect(keepPeekOnClick({ metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, button: 0 })).toBe(false);
  });
});

describe("IssuePeek loading and error", () => {
  it("shows the skeleton with only Close interactive, then hydrates", async () => {
    const { releaseDetail } = renderPeek({ holdDetail: true, details: {} });
    const panel = await screen.findByTestId("issue-preview");
    expect(within(panel).getByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    expect(within(panel).queryByRole("textbox", { name: "Work item title" })).not.toBeInTheDocument();
    expect(screen.queryByText(PEEK_MISSING_TITLE)).not.toBeInTheDocument();
    expect(within(panel).getAllByRole("button")).toHaveLength(1);
    expect(within(panel).getByRole("button", { name: "Close preview" })).toBeInTheDocument();
    releaseDetail();
    expect(await screen.findByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(screen.getByText("WEB-1")).toBeInTheDocument();
  });

  it("collapses a missing issue into the generic error with only Close", async () => {
    const { onClose } = renderPeek({ detailStatus: 404, details: {} });
    // The error replaces the skeleton with a fresh panel node; scope the
    // assertions to the settled tree, never the loading capture.
    expect(await screen.findByRole("heading", { name: PEEK_MISSING_TITLE })).toBeInTheDocument();
    const panel = screen.getByTestId("issue-preview");
    expect(within(panel).getByText(PEEK_MISSING_BODY)).toBeInTheDocument();
    expect(within(panel).queryByRole("textbox", { name: "Work item title" })).not.toBeInTheDocument();
    expect(within(panel).getAllByRole("button")).toHaveLength(1);
    fireEvent.click(within(panel).getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("collapses a refused issue into the same error", async () => {
    renderPeek({ detailStatus: 403, details: {} });
    expect(await screen.findByRole("heading", { name: PEEK_MISSING_TITLE })).toBeInTheDocument();
  });

  it("renders the error when the detail resolves empty", async () => {
    renderPeek({ details: {}, emptyDetail: true, detailStatus: 404 });
    expect(await screen.findByRole("heading", { name: PEEK_MISSING_TITLE })).toBeInTheDocument();
  });
});

describe("IssuePeek loaded", () => {
  it("composes the header actions, title, description, and widgets", async () => {
    renderPeek({ details: {}, creator: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    // Hydration remounts the portal, so re-read the settled panel node.
    const panel = screen.getByTestId("issue-preview");
    expect(within(panel).getByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(within(panel).getByRole("button", { name: "WEB-1" })).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Copy work item link" })).toBeInTheDocument();
    expect(await within(panel).findByRole("button", { name: "Subscribe" })).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Work item actions" })).toBeInTheDocument();
    expect(within(panel).getByTestId("peek-fullscreen-link")).toHaveAttribute("href", "/acme/browse/WEB-1");
    expect(await within(panel).findByText("Current words")).toBeInTheDocument();
    expect(await within(panel).findByTestId("issue-detail-widgets")).toBeInTheDocument();
    expect(within(panel).getByRole("region", { name: "Properties" })).toBeInTheDocument();
  });

  it("commits title edits through the item update with the shared pipeline", async () => {
    const user = userEvent.setup();
    const { writes } = renderPeek({ details: {} });
    const field = await screen.findByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Renamed issue{Enter}");
    await waitFor(() => {
      expect(writes.some((write) => write.method === "PATCH" && String(write.body).includes("Renamed issue"))).toBe(
        true
      );
    });
    expect(await screen.findByText("Saved")).toBeInTheDocument();
  });

  it("reports a refused title save and settles the pipeline", async () => {
    const user = userEvent.setup();
    renderPeek({ details: {}, patchStatus: 500 });
    const field = await screen.findByRole("textbox", { name: "Work item title" });
    await user.clear(field);
    await user.type(field, "Refused rename{Enter}");
    expect(await screen.findByText("Saved")).toBeInTheDocument();
  });

  it("mirrors the detail sidebar and saves a property edit", async () => {
    const { writes } = renderPeek({ details: {}, creator: true });
    const properties = await screen.findByRole("region", { name: "Properties" });
    expect(await within(properties).findByRole("group", { name: "State" })).toHaveTextContent("Done");
    expect(within(properties).getByRole("group", { name: "Priority" })).toHaveTextContent("High");
    expect(within(properties).getByRole("group", { name: "Created by" })).toHaveTextContent("Ada L");
    fireEvent.click(within(within(properties).getByRole("group", { name: "Priority" })).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Low" }));
    await waitFor(() => {
      expect(writes.some((write) => write.method === "PATCH" && String(write.body).includes('"priority":"low"'))).toBe(
        true
      );
    });
  });

  it("routes the action row Attach entry to the attachment picker", async () => {
    renderPeek({ details: {} });
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

  it("refreshes the detail after a subscription change", async () => {
    const user = userEvent.setup();
    const { writes } = renderPeek({ details: {} });
    await user.click(await screen.findByRole("button", { name: "Subscribe" }));
    await waitFor(() => {
      expect(writes.some((write) => write.method === "POST" && write.url.endsWith("/subscribe/"))).toBe(true);
    });
    expect(await screen.findByRole("button", { name: "Unsubscribe" })).toBeInTheDocument();
  });
});

describe("IssuePeek modes", () => {
  it("docks at the side by default and reshapes through the toggle", async () => {
    const user = userEvent.setup();
    renderPeek({ details: {} });
    await screen.findByRole("textbox", { name: "Work item title" });
    expect(screen.getByTestId("issue-preview")).toHaveAttribute("data-peek-layout", "side");
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    await user.click(await screen.findByRole("menuitem", { name: "Modal" }));
    expect(screen.getByTestId("issue-preview")).toHaveAttribute("data-peek-layout", "modal");
    expect(screen.getByTestId("peek-backdrop")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(screen.getByTestId("issue-preview")).toHaveAttribute("data-peek-layout", "full");
    expect(screen.queryByTestId("peek-backdrop")).not.toBeInTheDocument();
  });

  it("lays a single column except in full screen", async () => {
    const user = userEvent.setup();
    renderPeek({ details: {} });
    expect(await screen.findByRole("region", { name: "Properties" })).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "Properties" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Display mode" }));
    await user.click(await screen.findByRole("menuitem", { name: "Full screen" }));
    expect(await screen.findByRole("complementary", { name: "Properties" })).toBeInTheDocument();
  });
});

describe("IssuePeek close", () => {
  it("closes from the button, Escape, and outside presses", async () => {
    const first = renderPeek({ details: {} });
    await screen.findByRole("textbox", { name: "Work item title" });
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(first.onClose).toHaveBeenCalledTimes(1);
    first.unmount();

    const second = renderPeek({ details: {} });
    await screen.findByRole("textbox", { name: "Work item title" });
    fireEvent.keyDown(document, { key: "Escape" });
    expect(second.onClose).toHaveBeenCalledTimes(1);
    fireEvent.pointerDown(document.body);
    expect(second.onClose).toHaveBeenCalledTimes(2);
    second.unmount();
  });

  it("refocuses the originating row on unmount", async () => {
    const { setOpen } = renderPeek({ details: {}, withOrigin: true, startClosed: true });
    const origin = await screen.findByRole("button", { name: "Origin row" });
    origin.focus();
    act(() => setOpen(true));
    await screen.findByRole("textbox", { name: "Work item title" });
    act(() => setOpen(false));
    expect(document.activeElement).toBe(origin);
  });

  it("leaves focus alone when nothing focusable opened the peek", async () => {
    const active = vi.spyOn(document, "activeElement", "get").mockReturnValue(null);
    const { unmount } = renderPeek({ details: {} });
    await screen.findByTestId("issue-preview");
    active.mockRestore();
    unmount();
    expect(document.activeElement).toBe(document.body);
  });

  it("disables Escape and outside-click close when embedded", async () => {
    const { onClose } = renderPeek({ details: {}, embedded: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.pointerDown(document.body);
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("IssuePeek fullscreen link", () => {
  it("clears the peek before navigating to the full page", async () => {
    const user = userEvent.setup();
    const { onClose, router } = renderPeek({ details: {} });
    const link = await screen.findByTestId("peek-fullscreen-link");
    await user.click(link);
    expect(onClose).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(router.state.location.pathname).toBe(`/${SLUG}/browse/WEB-1`);
    });
    expect(await screen.findByText("full page")).toBeInTheDocument();
  });

  it("leaves the peek up on modified clicks", async () => {
    const { onClose, router } = renderPeek({ details: {} });
    const link = await screen.findByTestId("peek-fullscreen-link");
    fireEvent.click(link, { metaKey: true });
    expect(onClose).not.toHaveBeenCalled();
    expect(router.state.location.pathname).toBe(`/${SLUG}/projects/${PROJECT}/issues`);
  });
});

describe("IssuePeek quick actions", () => {
  it("closes the peek after a landed delete", async () => {
    const user = userEvent.setup();
    const { onClose, writes } = renderPeek({ details: {}, creator: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    await user.click(screen.getByRole("button", { name: "Work item actions" }));
    await user.click(await screen.findByRole("menuitem", { name: /delete/i }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: /delete/i });
    await user.click(confirm);
    await waitFor(() => {
      expect(writes.some((write) => write.method === "DELETE" && write.url.includes(`/issues/${ISSUE}/`))).toBe(true);
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("closes the peek after a landed archive", async () => {
    const user = userEvent.setup();
    const { onClose, writes } = renderPeek({ details: {}, creator: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    await user.click(screen.getByRole("button", { name: "Work item actions" }));
    await user.click(await screen.findByRole("menuitem", { name: /archive/i }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: /archive/i });
    await user.click(confirm);
    await waitFor(() => {
      expect(writes.some((write) => write.method === "POST" && write.url.endsWith("/archive/"))).toBe(true);
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("closes the peek after a landed restore", async () => {
    const user = userEvent.setup();
    const { onClose, writes } = renderPeek({ details: {}, creator: true, archivedIssue: true });
    await screen.findByTestId("issue-preview");
    await user.click(await screen.findByRole("button", { name: "Work item actions" }));
    await user.click(await screen.findByRole("menuitem", { name: /restore/i }));
    await waitFor(() => {
      expect(writes.some((write) => write.method === "DELETE" && write.url.endsWith("/archive/"))).toBe(true);
    });
    expect(onClose).toHaveBeenCalled();
  });
});

describe("IssuePeek nesting", () => {
  it("stacks a child peek above and writes the nesting level", async () => {
    const user = userEvent.setup();
    const { onClose, router } = renderPeek({ details: {}, child: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    // The nested skeleton remounts into the loaded panel; wait on the
    // live tree and re-read the settled node.
    await waitFor(() => {
      const live = screen.getByTestId(NESTED_PEEK_TEST_ID);
      expect(within(live).queryByRole("textbox", { name: "Work item title" })).not.toBeNull();
    });
    const nested = screen.getByTestId(NESTED_PEEK_TEST_ID);
    expect(within(nested).getByRole("textbox", { name: "Work item title" })).toHaveValue("Every inspector");
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    await waitFor(() => {
      expect(router.state.location.search).toMatchObject({ peekNestingLevel: "1" });
    });
    fireEvent.click(within(nested).getByRole("button", { name: "Close preview" }));
    await waitFor(() => {
      expect(screen.queryByTestId(NESTED_PEEK_TEST_ID)).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(router.state.location.search).not.toHaveProperty("peekNestingLevel");
    });
  });

  it("holds the parent Escape while the child is up", async () => {
    const user = userEvent.setup();
    const { onClose } = renderPeek({ details: {}, child: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    await screen.findByTestId(NESTED_PEEK_TEST_ID);
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByTestId(NESTED_PEEK_TEST_ID)).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("keeps the parent up for presses inside the stacked child", async () => {
    const user = userEvent.setup();
    const { onClose } = renderPeek({ details: {}, child: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    await waitFor(() => {
      const live = screen.getByTestId(NESTED_PEEK_TEST_ID);
      expect(within(live).queryByRole("textbox", { name: "Work item title" })).not.toBeNull();
    });
    const nested = screen.getByTestId(NESTED_PEEK_TEST_ID);
    fireEvent.pointerDown(within(nested).getByRole("textbox", { name: "Work item title" }));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId(NESTED_PEEK_TEST_ID)).toBeInTheDocument();
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
  });

  it("closes the child on an outside press and leaves the parent up", async () => {
    const user = userEvent.setup();
    const { onClose } = renderPeek({ details: {}, child: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    await screen.findByTestId(NESTED_PEEK_TEST_ID);
    fireEvent.pointerDown(document.body);
    await waitFor(() => {
      expect(screen.queryByTestId(NESTED_PEEK_TEST_ID)).not.toBeInTheDocument();
    });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("reports the child and the return to a host that owns the address", async () => {
    const user = userEvent.setup();
    const onNestedChange = vi.fn();
    const { router } = renderPeek({ details: {}, child: true, onNestedChange });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    await screen.findByTestId(NESTED_PEEK_TEST_ID);
    expect(onNestedChange).toHaveBeenCalledWith({ issueId: CHILD, projectId: PROJECT, nestingLevel: 1 });
    // The host owns the address: the panel does not write the level itself.
    expect(router.state.location.search).not.toHaveProperty("peekNestingLevel");
    fireEvent.click(within(screen.getByTestId(NESTED_PEEK_TEST_ID)).getByRole("button", { name: "Close preview" }));
    await waitFor(() => {
      expect(screen.queryByTestId(NESTED_PEEK_TEST_ID)).not.toBeInTheDocument();
    });
    expect(onNestedChange).toHaveBeenLastCalledWith({ issueId: ISSUE, projectId: PROJECT, nestingLevel: 0 });
  });

  it("shows the nested skeleton while the child loads", async () => {
    const user = userEvent.setup();
    const { releaseChild } = renderPeek({ details: {}, child: true, holdChild: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    const nested = await screen.findByTestId(NESTED_PEEK_TEST_ID);
    expect(within(nested).getByRole("status", { name: "Loading work item" })).toBeInTheDocument();
    releaseChild();
    await waitFor(() => {
      const live = screen.getByTestId(NESTED_PEEK_TEST_ID);
      expect(within(live).queryByRole("textbox", { name: "Work item title" })).not.toBeNull();
    });
    expect(
      within(screen.getByTestId(NESTED_PEEK_TEST_ID)).getByRole("textbox", { name: "Work item title" })
    ).toHaveValue("Every inspector");
  });

  it("shows the nested error when the child is gone", async () => {
    const user = userEvent.setup();
    renderPeek({ details: {}, child: true, childStatus: 404 });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    await screen.findByTestId(NESTED_PEEK_TEST_ID);
    await waitFor(() => {
      const live = screen.getByTestId(NESTED_PEEK_TEST_ID);
      expect(within(live).queryByRole("heading", { name: PEEK_MISSING_TITLE })).not.toBeNull();
    });
  });
});

describe("ResolvingIssuePeek hosting", () => {
  it("keeps the root panel mounted while a nested child owns the address", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderPeek({ details: {}, child: true, resolve: true, issueKey: null });
    await screen.findByRole("textbox", { name: "Work item title" });
    const widgets = await screen.findByTestId("issue-detail-widgets");
    await user.click(await within(widgets).findByRole("link", { name: /Every inspector/ }));
    await waitFor(() => {
      expect(
        within(screen.getByTestId(NESTED_PEEK_TEST_ID)).getByRole("textbox", { name: "Work item title" })
      ).toHaveValue("Every inspector");
    });
    // The host re-mounted the resolving peek with the child as its address,
    // yet the root panel and its stack stay put.
    const root = screen.getByTestId("issue-preview");
    expect(within(root).getByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(screen.getByTestId(NESTED_PEEK_TEST_ID)).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.pointerDown(document.body);
    await waitFor(() => {
      expect(screen.queryByTestId(NESTED_PEEK_TEST_ID)).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("issue-preview")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("follows an address the mount did not open itself", async () => {
    const user = userEvent.setup();
    renderPeek({ details: {}, resolve: true });
    await screen.findByRole("textbox", { name: "Work item title" });
    await user.click(screen.getByRole("button", { name: "Move address to child" }));
    // No child opened here, so the mount drops its stack and reads the
    // new address: the child becomes the root panel.
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Every inspector");
    });
    expect(screen.queryByTestId(NESTED_PEEK_TEST_ID)).not.toBeInTheDocument();
  });
});

describe("IssuePeek standing", () => {
  it("reads only for guests with a heading title", async () => {
    renderPeek({ details: { member_role: null } });
    expect(await screen.findByRole("heading", { name: "Fix login" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Work item title" })).not.toBeInTheDocument();
  });

  it("reads only on archived issues", async () => {
    renderPeek({ details: {}, archivedIssue: true });
    expect(await screen.findByRole("heading", { name: "Fix login" })).toBeInTheDocument();
  });

  it("shows an archived item's set estimate read-only (NEWFRONT-349 intended, EST-022)", async () => {
    renderPeek({ details: {}, archivedIssue: true, estimates: true, estimatePoint: ESTIMATE_POINT_TWO });
    await screen.findByTestId("issue-preview");
    const row = await screen.findByRole("group", { name: "Estimate" });
    const trigger = within(row).getByRole("button");
    await waitFor(() => expect(trigger).toHaveTextContent("2"));
    expect(trigger).toBeDisabled();
    fireEvent.click(trigger);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.queryAllByRole("textbox", { name: "Work item title" })).toHaveLength(0);
  });

  it("shows a guest's set estimate read-only in the peek (EST-022)", async () => {
    renderPeek({
      details: { member_role: null },
      estimates: true,
      estimatePoint: ESTIMATE_POINT_ONE,
    });
    await screen.findByRole("heading", { name: "Fix login" });
    const row = await screen.findByRole("group", { name: "Estimate" });
    const trigger = within(row).getByRole("button");
    await waitFor(() => expect(trigger).toHaveTextContent("1"));
    expect(trigger).toBeDisabled();
  });

  it("shows the empty placeholder on an unestimated peek (EST-022)", async () => {
    renderPeek({ details: {}, estimates: true, estimatePoint: null });
    await screen.findByTestId("issue-preview");
    const row = await screen.findByRole("group", { name: "Estimate" });
    expect(within(row).getByRole("button")).toHaveTextContent("None");
  });

  it("reads only on archived projects", async () => {
    renderPeek({ details: { archived_at: "2024-03-04T05:06:07Z" } });
    expect(await screen.findByRole("heading", { name: "Fix login" })).toBeInTheDocument();
  });

  it("hides the menu on synced issues", async () => {
    renderPeek({ details: {}, syncedIssue: true });
    expect(await screen.findByRole("button", { name: "Copy work item link" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Work item actions" })).not.toBeInTheDocument();
  });

  it("renders an epic peek with its own widget title", async () => {
    renderPeek({ details: {}, epic: true, creator: true });
    expect(await screen.findByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(await screen.findByText("Work items")).toBeInTheDocument();
  });

  it("renders on published projects", async () => {
    renderPeek({ details: {}, published: true });
    expect(await screen.findByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
  });

  it("renders while the workspace read is unresolved", async () => {
    renderPeek({ details: {}, unresolvedWorkspace: true });
    expect(await screen.findByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
  });
});

describe("IssuePeek deep links", () => {
  it("derives the key from the loaded project for UUID links", async () => {
    renderPeek({ details: {}, issueKey: null });
    expect(await screen.findByRole("textbox", { name: "Work item title" })).toHaveValue("Fix login");
    expect(screen.getByRole("button", { name: "WEB-1" })).toBeInTheDocument();
    expect(screen.getByTestId("peek-fullscreen-link")).toHaveAttribute("href", "/acme/browse/WEB-1");
  });

  it("falls back to the opener key prefix without project details", async () => {
    renderPeek({ details: "error", projectStatus: 500 });
    expect(await screen.findByTestId("peek-fullscreen-link")).toHaveAttribute("href", "/acme/browse/WEB-1");
    expect(screen.getByRole("heading", { name: "Fix login" })).toBeInTheDocument();
  });

  it("keeps the full-screen link disabled without a key or a loaded project", async () => {
    renderPeek({ details: "error", projectStatus: 500, issueKey: null, projectIdentifier: "ZZZ" });
    const link = await screen.findByTestId("peek-fullscreen-link");
    expect(link).toBeDisabled();
    expect(link).not.toHaveAttribute("href");
  });

  it("enables the full-screen link once the project code loads for a UUID link", async () => {
    renderPeek({ details: {}, issueKey: null });
    const link = await screen.findByTestId("peek-fullscreen-link");
    expect(link).not.toBeDisabled();
    expect(link).toHaveAttribute("href", "/acme/browse/WEB-1");
  });
});
