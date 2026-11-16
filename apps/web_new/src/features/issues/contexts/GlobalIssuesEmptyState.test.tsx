// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues empty surface (parity/specs/issues.md §8, ISS-073).
// With no visible project the card asks for one; with a project but no
// rows it asks for a work item; either entry opens its dialog, and a guest
// sees the same copy with the entries dead.
import type { Project } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ canEdit: true }));

vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  usePermissions: () => ({
    role: mocks.canEdit ? 20 : null,
    isMember: mocks.canEdit,
    canEdit: mocks.canEdit,
    isAdmin: false,
    isLoading: false,
  }),
}));

vi.mock("../../projects/components/ProjectCreateEntry.js", () => ({
  ProjectCreateEntry: (props: { open: boolean; workspaceSlug: string }) =>
    props.open ? <div role="dialog" aria-label="Create project" data-ws={props.workspaceSlug} /> : null,
}));

vi.mock("../components/WorkItemCreateDialog.js", () => ({
  WorkItemCreateDialog: (props: { open: boolean; projects: Array<{ id: string }> }) =>
    props.open ? (
      <div
        role="dialog"
        aria-label="Create new work item"
        data-projects={props.projects.map((row) => row.id).join(",")}
      />
    ) : null,
}));

import { GlobalIssuesEmptyState } from "./GlobalIssuesEmptyState.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const USER = "123e4567-e89b-12d3-a456-426614174000";

function project(overrides: Partial<Project> = {}): Project {
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
    cycle_view: false,
    module_view: false,
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

function renderState(projects: Project[]) {
  render(<GlobalIssuesEmptyState workspaceSlug={SLUG} projects={projects} />);
}

beforeEach(() => {
  mocks.canEdit = true;
});

describe("GlobalIssuesEmptyState", () => {
  it("asks for a project when the workspace shows none and opens the dialog (ISS-073)", async () => {
    const user = userEvent.setup();
    renderState([]);
    expect(await screen.findByText("No project")).toBeInTheDocument();
    expect(screen.getByTestId("issues-empty-state")).toHaveAttribute("data-context", "global-no-project");
    await user.click(screen.getByRole("button", { name: "Start your first project" }));
    expect(await screen.findByRole("dialog", { name: "Create project" })).toHaveAttribute("data-ws", SLUG);
  });

  it("asks for a work item once a project exists and opens the create dialog", async () => {
    const user = userEvent.setup();
    renderState([project()]);
    expect(await screen.findByText("No Views yet")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add work item" }));
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toHaveAttribute(
      "data-projects",
      PROJECT
    );
  });

  it("disables the work-item entry for a workspace guest", async () => {
    mocks.canEdit = false;
    renderState([project()]);
    expect(await screen.findByRole("button", { name: "Add work item" })).toBeDisabled();
  });

  it("disables the project entry for a guest with no project", async () => {
    mocks.canEdit = false;
    renderState([]);
    expect(await screen.findByRole("button", { name: "Start your first project" })).toBeDisabled();
  });

  it("enables the work-item entry for a member even with no creatable project (ISS-073)", async () => {
    // Workspace standing alone gates the entry; the dialog states the
    // missing project itself once opened.
    const user = userEvent.setup();
    renderState([project({ member_role: 5 })]);
    expect(await screen.findByText("No Views yet")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add work item" }));
    expect(await screen.findByRole("dialog", { name: "Create new work item" })).toHaveAttribute("data-projects", "");
  });
});
