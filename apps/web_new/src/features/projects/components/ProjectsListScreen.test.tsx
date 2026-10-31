// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the screen composition.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: { params?: Record<string, string>; children?: React.ReactNode }) => (
    <a href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}>{props.children}</a>
  ),
}));

import { ProjectsListScreen, selectListBody, selectViewRows, type CreateDialogControls } from "./ProjectsListScreen.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function project(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name: "Website relaunch",
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

describe("selectViewRows", () => {
  const active = project({ id: "223e4567-e89b-12d3-a456-426614174001", archived_at: null });
  const archived = project({ id: "323e4567-e89b-12d3-a456-426614174002", archived_at: "2024-05-06T07:08:09Z" });

  it("keeps active rows on the main view", () => {
    expect(selectViewRows([active, archived], false)).toEqual([active]);
  });

  it("keeps archived rows on the archive view", () => {
    expect(selectViewRows([active, archived], true)).toEqual([archived]);
  });
});

describe("selectListBody", () => {
  const base = { collection: [project()], rowCount: 1, searchQuery: "", hasActiveFilters: false, archived: false };

  it("loads while the collection resolves", () => {
    expect(selectListBody({ ...base, collection: undefined, rowCount: 0 })).toBe("loading");
  });

  it("grids when rows render", () => {
    expect(selectListBody(base)).toBe("grid");
  });

  it("misses on a search with no rows", () => {
    expect(selectListBody({ ...base, rowCount: 0, searchQuery: "zzz" })).toBe("no-match");
  });

  it("treats blank searches as no search", () => {
    expect(selectListBody({ ...base, rowCount: 0, searchQuery: "   " })).toBe("first-run");
  });

  it("misses when filters narrow everything out", () => {
    expect(selectListBody({ ...base, rowCount: 0, hasActiveFilters: true })).toBe("no-match");
  });

  it("congratulates an empty archive", () => {
    expect(selectListBody({ ...base, rowCount: 0, archived: true })).toBe("empty-archive");
  });

  it("welcomes first runs", () => {
    expect(selectListBody({ ...base, rowCount: 0 })).toBe("first-run");
  });
});

describe("ProjectsListScreen", () => {
  it("shows skeleton cards while resolving, never an empty flash", () => {
    render(<ProjectsListScreen workspaceSlug="acme" collection={undefined} canCreate={true} />);

    expect(screen.getByTestId("projects-skeleton")).not.toBeNull();
    expect(screen.queryByTestId("projects-grid")).toBeNull();
    expect(screen.queryByTestId("projects-empty-state")).toBeNull();
  });

  it("grids the active rows on the main view", () => {
    const rows = [
      project({ id: "223e4567-e89b-12d3-a456-426614174001", name: "Alpha" }),
      project({ id: "323e4567-e89b-12d3-a456-426614174002", name: "Shelved", archived_at: "2024-05-06T07:08:09Z" }),
    ];
    render(<ProjectsListScreen workspaceSlug="acme" collection={rows} canCreate={true} />);

    expect(screen.getByLabelText("Projects")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Alpha" })).not.toBeNull();
    expect(screen.queryByRole("heading", { name: "Shelved" })).toBeNull();
    expect(screen.getByTestId("projects-grid").className).toContain("xl:grid-cols-3");
  });

  it("grids the archived rows on the archive view", () => {
    const rows = [
      project({ id: "223e4567-e89b-12d3-a456-426614174001", name: "Alpha" }),
      project({ id: "323e4567-e89b-12d3-a456-426614174002", name: "Shelved", archived_at: "2024-05-06T07:08:09Z" }),
    ];
    render(<ProjectsListScreen workspaceSlug="acme" archived={true} collection={rows} canCreate={true} />);

    expect(screen.getByLabelText("Archived projects")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Shelved" })).not.toBeNull();
    expect(screen.queryByRole("heading", { name: "Alpha" })).toBeNull();
  });

  it("prefers display-filtered rows when the filter slice passes them", () => {
    const rows = [project({ name: "Alpha" }), project({ id: "323e4567-e89b-12d3-a456-426614174002", name: "Beta" })];
    render(<ProjectsListScreen workspaceSlug="acme" collection={rows} projects={[rows[1]!]} canCreate={true} />);

    expect(screen.queryByRole("heading", { name: "Alpha" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Beta" })).not.toBeNull();
  });

  it("welcomes first runs without flashing the grid", () => {
    render(<ProjectsListScreen workspaceSlug="acme" collection={[]} canCreate={true} />);

    expect(screen.getByRole("heading", { name: "No active projects" })).not.toBeNull();
    expect(screen.queryByTestId("projects-grid")).toBeNull();
    expect(screen.queryByTestId("projects-skeleton")).toBeNull();
  });

  it("misses on a search with no rows", () => {
    render(
      <ProjectsListScreen
        workspaceSlug="acme"
        collection={[project()]}
        projects={[]}
        searchQuery="zzz"
        canCreate={true}
      />
    );

    expect(screen.getByRole("heading", { name: "No matching results." })).not.toBeNull();
  });

  it("congratulates an empty archive", () => {
    render(<ProjectsListScreen workspaceSlug="acme" archived={true} collection={[]} canCreate={true} />);

    expect(screen.getByRole("heading", { name: "No projects archived" })).not.toBeNull();
  });

  it("renders custom cards when a card renderer is passed", () => {
    render(
      <ProjectsListScreen
        workspaceSlug="acme"
        collection={[project({ name: "Alpha" })]}
        canCreate={true}
        renderCard={(row) => <article data-testid="custom-card">{row.name}</article>}
      />
    );

    expect(screen.getByTestId("custom-card")).toHaveTextContent("Alpha");
  });

  it("renders the filter pills strip above the body", () => {
    render(
      <ProjectsListScreen
        workspaceSlug="acme"
        collection={[]}
        canCreate={true}
        filterPills={<div data-testid="pills">2 filters</div>}
      />
    );

    expect(screen.getByTestId("pills")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "No active projects" })).not.toBeNull();
  });

  it("opens the creation dialog from the empty-state action", () => {
    const dialog = ({ open, onOpenChange }: CreateDialogControls): React.ReactNode =>
      open ? (
        <div data-testid="create-dialog">
          <button type="button" onClick={() => onOpenChange(false)}>
            Close
          </button>
        </div>
      ) : null;
    render(<ProjectsListScreen workspaceSlug="acme" collection={[]} canCreate={true} renderCreateDialog={dialog} />);

    expect(screen.queryByTestId("create-dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(screen.getByTestId("create-dialog")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByTestId("create-dialog")).toBeNull();
  });

  it("opens the creation dialog from the header button", () => {
    render(
      <ProjectsListScreen
        workspaceSlug="acme"
        collection={[project({ name: "Alpha" })]}
        canCreate={true}
        renderCreateDialog={({ open }) => (open ? <div data-testid="create-dialog">new project</div> : null)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /Project/ }));
    expect(screen.getByTestId("create-dialog")).not.toBeNull();
  });
});
