// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Surviving a refused removal (parity/specs/project-settings-members.md
// §"Remove and leave-self", §"Loading and failure states"; PSET-024,
// PSET-027; NEWFRONT-406). The membership under the confirmation has
// already been deleted out of band, so the DELETE answers 404. The old
// app answered that refusal with a burst of follow-up 404s and then took
// the page down; the new tab must simply stay standing: the confirmation
// withdraws, one DELETE is sent and nothing is re-read, no route change
// happens, the roster is untouched, and the tab keeps taking input.
import { ToastHost } from "@pidash/kit";
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerMocks = vi.hoisted(() => ({ navigate: vi.fn(), pathname: "/acme/settings/projects/p/members/" }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerMocks.navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: routerMocks.pathname } }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { createQueryClient } from "../../../core/query/client.js";
import { getToastManager } from "../../../shared/shell/toaster.js";
import { clearMemberFilters } from "./filters.js";
import { MembersSettings } from "./MembersSettings.js";
import { REMOVE_MEMBER_FAILED_TITLE } from "./api.js";

const SLUG = "acme";
const WORKSPACE = "123e4567-e89b-12d3-a456-4266141740f0";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";
const GRACE_MEMBERSHIP = "323e4567-e89b-12d3-a456-426614174022";

/** The server's wording for a membership that no longer resolves. */
const MEMBERSHIP_GONE = "Project membership not found.";

const MEMBERSHIPS = [
  {
    id: "323e4567-e89b-12d3-a456-426614174021",
    role: 20,
    member: ADA,
    project: PROJECT,
    original_role: 20,
    created_at: "2025-01-10T09:00:00Z",
  },
  {
    id: GRACE_MEMBERSHIP,
    role: 15,
    member: GRACE,
    project: PROJECT,
    original_role: 15,
    created_at: "2025-03-02T09:00:00Z",
  },
];

function directoryEntry(id: string, first: string, last: string, display: string, email: string) {
  return {
    id: `${id.slice(0, -1)}9`,
    role: 15,
    member: {
      id,
      first_name: first,
      last_name: last,
      avatar: "",
      avatar_url: null,
      display_name: display,
      is_bot: false,
      email,
    },
  };
}

const DIRECTORY = [
  directoryEntry(ADA, "Ada", "Lovelace", "ada", "ada@example.test"),
  directoryEntry(GRACE, "Grace", "Hopper", "Grace H", "grace@example.test"),
];

const ME = {
  id: ADA,
  email: "ada@example.test",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "ada",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2025-01-01T00:00:00Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

const OWN_MEMBERSHIP = {
  id: "423e4567-e89b-12d3-a456-4266141740aa",
  workspace: WORKSPACE,
  member: ADA,
  role: 20,
  company_role: null,
  view_props: {},
  default_props: {},
  issue_props: {},
  is_active: true,
  getting_started_checklist: {},
  tips: {},
  explored_features: {},
  draft_issue_count: 0,
  created_by: null,
  updated_by: null,
  created_at: "2025-01-01T00:00:00Z",
  updated_at: "2025-01-01T00:00:00Z",
};

const PROJECT_ROW = {
  id: PROJECT,
  created_at: "2025-01-01T00:00:00Z",
  updated_at: "2025-01-01T00:00:00Z",
  created_by: null,
  updated_by: null,
  deleted_at: null,
  workspace: WORKSPACE,
  name: "Alpha",
  description: "",
  description_text: null,
  description_html: null,
  network: 0,
  identifier: "ALP",
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
  archive_in: 1,
  close_in: 1,
  logo_props: {},
  default_state: null,
  archived_at: null,
  timezone: "UTC",
  external_source: null,
  external_id: null,
  repo_url: "",
  base_branch: "main",
  is_favorite: false,
  sort_order: null,
  member_role: 20,
  anchor: null,
  members: [ADA, GRACE],
  next_work_item_sequence: 1,
  agent_executor_options: [],
  default_agent_executor: "",
};

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  const payload = JSON.stringify(body);
  return {
    status,
    url,
    headers: { get: (name: string) => (name.toLowerCase() === "content-type" ? "application/json" : null) },
    text: async () => payload,
  };
}

interface RemovalTraffic {
  /** DELETE calls against the doomed membership. */
  deletes: number;
  /** Every request that started after that DELETE answered. */
  afterDelete: string[];
}

/**
 * The tab's own reads serve normally; only the doomed membership's DELETE
 * is refused. Any request the UI fires after that refusal is recorded so a
 * refetch storm shows up as traffic instead of a silently closed page.
 */
function refusingRemovalTransport(traffic: RemovalTraffic): Transport {
  let refused = false;
  return async (url: string, init): Promise<TransportResponse> => {
    if (refused) traffic.afterDelete.push(`${init.method} ${url}`);
    if (init.method === "DELETE" && url.endsWith(`/projects/${PROJECT}/members/${GRACE_MEMBERSHIP}/`)) {
      refused = true;
      traffic.deletes += 1;
      return jsonResponse(url, 404, { detail: MEMBERSHIP_GONE });
    }
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "token" });
    if (url.endsWith(`/workspaces/${SLUG}/workspace-members/me/`)) return jsonResponse(url, 200, OWN_MEMBERSHIP);
    if (url.endsWith(`/users/me/workspaces/${SLUG}/project-roles/`)) return jsonResponse(url, 200, { [PROJECT]: 20 });
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 200, ME);
    if (url.endsWith(`/workspaces/${SLUG}/members/`)) return jsonResponse(url, 200, DIRECTORY);
    if (url.includes(`/projects/${PROJECT}/members/`)) return jsonResponse(url, 200, MEMBERSHIPS);
    if (url.endsWith(`/workspaces/${SLUG}/projects/details/`)) return jsonResponse(url, 200, [PROJECT_ROW]);
    throw new Error(`unexpected request ${init.method} ${url}`);
  };
}

function renderTab(): void {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <ToastHost manager={getToastManager()} />
      <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={() => null} />
    </QueryClientProvider>
  );
}

function rosterUserIds(): string[] {
  return screen.queryAllByTestId("member-roster-row").map((row) => row.getAttribute("data-user-id") ?? "");
}

async function settleEntry(): Promise<void> {
  await waitFor(() => expect(rosterUserIds()).toEqual([ADA, GRACE]), { timeout: 5000 });
}

beforeEach(() => {
  clearMemberFilters();
});

afterEach(() => {
  resetAppClient();
  clearMemberFilters();
  vi.clearAllMocks();
});

describe("MembersSettings after a refused removal (NEWFRONT-406)", () => {
  it("keeps the tab standing and interactive when the membership is already gone", async () => {
    const traffic: RemovalTraffic = { deletes: 0, afterDelete: [] };
    createAppClient("https://api.test", refusingRemovalTransport(traffic));
    const user = userEvent.setup();
    renderTab();
    await settleEntry();

    const graceRow = () =>
      screen.getAllByTestId("member-roster-row").find((row) => row.getAttribute("data-user-id") === GRACE)!;
    await user.click(within(graceRow()).getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    // The toast carries role="dialog" too, so the confirmation is tracked
    // by its own accessible name from here on.
    const confirmName = "Remove Grace Hopper?";
    const confirmation = await screen.findByRole("dialog", { name: confirmName });
    await user.click(within(confirmation).getByRole("button", { name: "Remove" }));

    // One DELETE, answered 404; the confirmation withdraws instead of
    // hanging over a membership that no longer exists.
    await waitFor(() => expect(screen.queryByRole("dialog", { name: confirmName })).not.toBeInTheDocument(), {
      timeout: 5000,
    });
    expect(traffic.deletes).toBe(1);

    // Nothing is re-read and no route change happens: the refusal is an
    // event, not a reason to rebuild the tab. (The old app's storm of
    // follow-up 404s is what preceded its page death.)
    await waitFor(() => expect(screen.getByTestId("toast-notification")).toBeInTheDocument());
    expect(traffic.afterDelete).toEqual([]);
    expect(routerMocks.navigate).not.toHaveBeenCalled();

    // The tab is still there, with both rows, and still takes input.
    expect(screen.getByTestId("project-settings-members")).toBeInTheDocument();
    expect(rosterUserIds()).toEqual([ADA, GRACE]);
    const search = screen.getByRole("textbox", { name: "Search members" });
    await user.type(search, "grace");
    expect(rosterUserIds()).toEqual([GRACE]);
    await user.clear(search);
    expect(rosterUserIds()).toEqual([ADA, GRACE]);

    // The refusal is reported by the notice naming the action and the
    // server's message (NEWFRONT-398), and the notice is still up after
    // the confirmation is gone.
    const notice = screen.getByTestId("toast-notification");
    expect(within(notice).getByText(REMOVE_MEMBER_FAILED_TITLE)).toBeInTheDocument();
    expect(within(notice).getByText(MEMBERSHIP_GONE)).toBeInTheDocument();
    expect(traffic.afterDelete).toEqual([]);
  });
});
