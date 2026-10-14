// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails, Workspace } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the view wiring.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: { params?: Record<string, string>; children?: React.ReactNode }) => (
    <a href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}>{props.children}</a>
  ),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/index.js";
import { projectKeys } from "../api/keys.js";
import { ProjectsListView } from "./ProjectsListView.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

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
    agent_executor_options: null,
    ...overrides,
  };
}

function workspace(role: number): Workspace {
  return { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 2, role };
}

function renderView(args: { details?: ProjectDetails[]; role?: number; archived?: boolean }): void {
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (args.details !== undefined) queryClient.setQueryData(projectKeys.details("acme"), args.details);
  queryClient.setQueryData(sessionKeys.workspaces, [workspace(args.role ?? 20)]);
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectsListView workspaceSlug="acme" {...(args.archived === undefined ? {} : { archived: args.archived })} />
    </QueryClientProvider>
  );
}

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
});
