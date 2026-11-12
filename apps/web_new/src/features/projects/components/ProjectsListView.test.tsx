// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import type { Me, ProjectDetails, Workspace, WorkspaceMember } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the view wiring.
const { navigateMock } = vi.hoisted(() => ({ navigateMock: vi.fn() }));
vi.mock("@tanstack/react-router", () => ({
  Link: (props: { params?: Record<string, string>; children?: React.ReactNode }) => (
    <a href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}>{props.children}</a>
  ),
  useNavigate: () => navigateMock,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/index.js";
import { projectKeys } from "../api/keys.js";
import { useListDisplayStore } from "../store/list-display.js";
import { ProjectsListView } from "./ProjectsListView.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const MEMBER_ID = "323e4567-e89b-12d3-a456-426614174002";

function project(name: string, overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name,
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
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
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function workspace(role: number): Workspace {
  return { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 2, role };
}

const ME_ID = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

function member(id: string, displayName: string): WorkspaceMember {
  return {
    id: `m-${id}`,
    member: {
      id,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: "",
      display_name: displayName,
      is_bot: false,
    },
    role: 10,
  };
}

function renderView(args: {
  details?: ProjectDetails[];
  role?: number;
  archived?: boolean;
  members?: WorkspaceMember[];
  me?: boolean;
  slug?: string;
  transport?: (
    url: string,
    init?: { method?: string }
  ) => Promise<{
    status: number;
    url: string;
    headers: { get: () => null };
    text: () => Promise<string>;
  }>;
}) {
  createAppClient("https://api.test", (args.transport ?? (() => new Promise<never>(() => {}))) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const slug = args.slug ?? "acme";
  if (args.details !== undefined) queryClient.setQueryData(projectKeys.details(slug), args.details);
  if (args.members !== undefined) queryClient.setQueryData(projectKeys.members(slug), args.members);
  if (args.me === true) queryClient.setQueryData(sessionKeys.me, { id: ME_ID, display_name: "Ada Lovelace" } as Me);
  queryClient.setQueryData(sessionKeys.workspaces, [workspace(args.role ?? 20)]);
  return render(
    <QueryClientProvider client={queryClient}>
      <ProjectsListView workspaceSlug={slug} {...(args.archived === undefined ? {} : { archived: args.archived })} />
    </QueryClientProvider>
  );
}

function cardNames(): string[] {
  return screen.getAllByRole("heading").map((heading) => heading.textContent ?? "");
}

function csrfThen(
  calls: string[],
  handler: (url: string, method: string) => { status: number; body: string }
): (
  url: string,
  init?: { method?: string }
) => Promise<{
  status: number;
  url: string;
  headers: { get: () => null };
  text: () => Promise<string>;
}> {
  return async (url: string, init?: { method?: string }) => {
    const method = init?.method ?? "GET";
    calls.push(`${method} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    const { status, body } = handler(url, method);
    return { status, url, headers: { get: () => null }, text: async () => body };
  };
}

beforeEach(() => {
  useListDisplayStore.setState({ displays: {}, searchQuery: "", activeWorkspace: null });
  navigateMock.mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("ProjectsListView", () => {
  it("renders skeletons while the collection resolves", () => {
    renderView({});

    expect(screen.getByTestId("projects-skeleton")).not.toBeNull();
    expect(screen.queryByTestId("projects-empty-state")).toBeNull();
  });

  it("renders the fetched collection as cards for editors", () => {
    renderView({ details: [project("Alpha")], role: 15 });

    expect(screen.getByRole("heading", { name: "Alpha" })).not.toBeNull();
    expect(screen.getByRole("button", { name: /Project/ })).not.toBeNull();
  });

  it("hides creation from guests and disables the empty-state action", () => {
    renderView({ details: [], role: 5 });

    expect(screen.queryByRole("button", { name: /Project/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Start your first project" })).toBeDisabled();
  });

  it("passes the archive view through to the screen", () => {
    renderView({
      archived: true,
      role: 20,
      details: [project("Shelved", { archived_at: "2024-05-06T07:08:09Z" })],
    });

    expect(screen.getByLabelText("Archived projects")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Shelved" })).not.toBeNull();
  });

  it("reorders the rows through the sort menu", () => {
    renderView({ details: [project("Zephyr"), project("Alpine")], role: 15 });

    const desktop = screen.getByTestId("projects-desktop-filters");
    fireEvent.click(within(desktop).getByTestId("projects-sort-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Name" }));

    expect(cardNames()).toEqual(["Alpine", "Zephyr"]);
    expect(useListDisplayStore.getState().displayFor("acme").sort).toEqual({
      key: "name",
      direction: "ascending",
    });
  });

  it("filters through the panel and clears from the strip", async () => {
    const user = userEvent.setup();
    renderView({
      details: [project("Strip Public", { network: 2 }), project("Strip Secret", { network: 0 })],
      role: 15,
    });

    const desktop = screen.getByTestId("projects-desktop-filters");
    await user.click(within(desktop).getByTestId("projects-filter-trigger"));
    await user.click(screen.getByText("Private"));
    expect(cardNames()).toEqual(["Strip Secret"]);
    expect(screen.getByTestId("projects-filter-count")).toHaveTextContent("1/2");

    await user.click(screen.getByRole("button", { name: "Remove Private" }));
    expect(cardNames()).toEqual(["Strip Public", "Strip Secret"]);

    await user.click(within(desktop).getByTestId("projects-filter-trigger"));
    await user.click(screen.getByText("Private"));
    await user.click(screen.getByRole("button", { name: "Clear all" }));
    expect(cardNames()).toEqual(["Strip Public", "Strip Secret"]);
    expect(screen.queryByTestId("projects-applied-filters")).toBeNull();
  });

  it("narrows to the caller's projects and leads", async () => {
    const user = userEvent.setup();
    renderView({
      details: [
        project("Mine", { member_role: 15, project_lead: ME_ID }),
        project("Theirs", { member_role: null, project_lead: "bbbbbbbb-cccc-dddd-eeee-ffffffffffff" }),
      ],
      members: [member(ME_ID, "Ada Lovelace"), member("bbbbbbbb-cccc-dddd-eeee-ffffffffffff", "Grace Hopper")],
      me: true,
      role: 15,
    });

    const desktop = screen.getByTestId("projects-desktop-filters");
    await user.click(within(desktop).getByTestId("projects-filter-trigger"));
    await user.click(screen.getByText("My projects"));
    expect(cardNames()).toEqual(["Mine"]);

    await user.click(screen.getByRole("button", { name: "Remove My projects" }));
    expect(cardNames()).toEqual(["Mine", "Theirs"]);

    await user.click(within(desktop).getByTestId("projects-filter-trigger"));
    const lead = screen.getByRole("region", { name: "Lead" });
    await user.click(within(lead).getByText("You"));
    expect(cardNames()).toEqual(["Mine"]);

    await user.click(screen.getByRole("button", { name: "Clear Lead filters" }));
    expect(cardNames()).toEqual(["Mine", "Theirs"]);
  });

  it("narrows by creation date", async () => {
    const user = userEvent.setup();
    renderView({
      details: [
        project("Fresh", { created_at: new Date().toISOString() }),
        project("Stale", { created_at: "2020-01-01T00:00:00Z" }),
      ],
      role: 15,
    });

    const desktop = screen.getByTestId("projects-desktop-filters");
    await user.click(within(desktop).getByTestId("projects-filter-trigger"));
    await user.click(screen.getByText("Last 30 days"));
    expect(cardNames()).toEqual(["Fresh"]);
    expect(screen.getByTestId("projects-filter-count")).toHaveTextContent("1/2");
  });

  it("searches live and drops the text on workspace switches", async () => {
    const user = userEvent.setup();
    const first = renderView({
      slug: "acme",
      details: [project("Findable Apple"), project("Hidden Banana")],
      role: 15,
    });

    await user.click(screen.getByRole("button", { name: "Search projects" }));
    await user.type(screen.getByPlaceholderText("Search"), "apple");
    expect(cardNames()).toEqual(["Findable Apple"]);
    expect(useListDisplayStore.getState().searchQuery).toBe("apple");

    first.unmount();
    renderView({ slug: "globex", details: [project("Findable Apple"), project("Hidden Banana")], role: 15 });
    expect(useListDisplayStore.getState().searchQuery).toBe("");
    expect(cardNames()).toEqual(["Findable Apple", "Hidden Banana"]);
  });

  it("keeps one workspace's display apart from another", () => {
    useListDisplayStore.getState().setSort("globex", { key: "name", direction: "descending" });
    renderView({ slug: "acme", details: [project("Bravo"), project("Alpha")], role: 15 });

    expect(cardNames()).toEqual(["Bravo", "Alpha"]);
    expect(useListDisplayStore.getState().displayFor("globex").sort.key).toBe("name");
  });

  it("stars for editors and resolves member names through the directory", () => {
    renderView({
      details: [project("Alpha", { members: [MEMBER_ID] })],
      members: [member(MEMBER_ID, "Ada Lovelace")],
      role: 15,
    });

    expect(screen.getByTestId("project-card-star")).toHaveAttribute("aria-label", "Add Alpha to favorites");
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).not.toBeNull();
  });

  it("hides the star from guests", () => {
    renderView({
      details: [project("Alpha", { members: [MEMBER_ID] })],
      members: [member(MEMBER_ID, "Ada")],
      role: 5,
    });

    expect(screen.queryByTestId("project-card-star")).toBeNull();
  });

  it("toggles the star through the favorite endpoint", async () => {
    const calls: string[] = [];
    renderView({
      details: [project("Alpha")],
      role: 15,
      transport: csrfThen(calls, (url, method) => {
        if (method === "POST" && url.includes("/user-favorite-projects/")) return { status: 204, body: "" };
        return { status: 404, body: "{}" };
      }),
    });

    fireEvent.click(screen.getByTestId("project-card-star"));
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorite-projects/"))).toBe(true)
    );
  });

  it("opens the shared create dialog from the header button", () => {
    renderView({
      details: [project("Alpha")],
      members: [member(MEMBER_ID, "Ada")],
      role: 15,
    });

    fireEvent.click(screen.getByRole("button", { name: /Project/ }));
    expect(screen.getByRole("heading", { name: "New project" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "Project lead" }).hasAttribute("disabled")).toBe(false);
  });

  it("opens the shared create dialog from the empty-state action", () => {
    renderView({ details: [], role: 15 });

    fireEvent.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(screen.getByRole("heading", { name: "New project" })).not.toBeNull();
  });

  it("holds the lead picker until members resolve", () => {
    renderView({ details: [], role: 15 });

    fireEvent.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(screen.getByRole("button", { name: "Project lead" }).hasAttribute("disabled")).toBe(true);
  });

  it("opens the archive dialog from the card menu", () => {
    renderView({ details: [project("Alpha", { member_role: 20 })], role: 20 });

    fireEvent.click(screen.getByTestId("project-card-actions"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(screen.getByRole("heading", { name: "Archive Alpha" })).not.toBeNull();
  });

  it("opens the restore dialog from the archived footer", () => {
    renderView({
      archived: true,
      role: 20,
      details: [project("Shelved", { archived_at: "2024-05-06T07:08:09Z", member_role: 20 })],
    });

    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(screen.getByRole("heading", { name: "Restore Shelved" })).not.toBeNull();
  });

  it("opens the delete dialog from the archived footer and closes it again", () => {
    renderView({
      archived: true,
      role: 20,
      details: [project("Shelved", { archived_at: "2024-05-06T07:08:09Z", member_role: 20 })],
    });
    expect(screen.queryByRole("heading", { name: "Delete project" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("heading", { name: "Delete project" })).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("heading", { name: "Delete project" })).toBeNull();
  });

  it("joins from the card, navigates inside, and flips the card to member state", async () => {
    const calls: string[] = [];
    const joined = project("Alpha", { member_role: 15 });
    renderView({
      details: [project("Alpha", { member_role: null })],
      members: [member(MEMBER_ID, "Ada")],
      role: 15,
      transport: csrfThen(calls, (url, method) => {
        if (method === "POST" && url.endsWith("/projects/invitations/")) {
          return { status: 201, body: JSON.stringify({ message: "joined" }) };
        }
        if (url.endsWith("/projects/details/")) return { status: 200, body: JSON.stringify([joined]) };
        if (url.endsWith("/members/")) return { status: 200, body: JSON.stringify([member(MEMBER_ID, "Ada")]) };
        return { status: 404, body: "{}" };
      }),
    });

    expect(screen.getByRole("button", { name: "Join" })).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    fireEvent.click(screen.getByRole("button", { name: "Join Project" }));
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith({ to: `/acme/projects/${PROJECT_ID}/issues` }));
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/projects/invitations/"))).toBe(true);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Join" })).toBeNull());
    expect(screen.getByRole("button", { name: "Alpha settings" })).not.toBeNull();
  });
});
