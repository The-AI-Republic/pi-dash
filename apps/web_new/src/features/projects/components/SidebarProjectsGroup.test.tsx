// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Project } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SidebarProjectsGroup, type SidebarProjectsGroupProps } from "./SidebarProjectsGroup.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_A = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_B = "323e4567-e89b-12d3-a456-426614174002";
const USER_ID = "423e4567-e89b-12d3-a456-426614174003";

function project(id: string, name: string, overrides: Partial<Project> = {}): Project {
  return {
    id,
    name,
    identifier: name.slice(0, 3).toUpperCase(),
    sort_order: 10000,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
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

const PROJECTS = [
  project(PROJECT_A, "Atlas", { sort_order: 10000 }),
  project(PROJECT_B, "Beacon", { sort_order: 20000 }),
];

function workspacePropertiesPayload(limit: number, mode: string): Record<string, unknown> {
  return {
    id: "523e4567-e89b-12d3-a456-426614174004",
    workspace: WS_ID,
    user: USER_ID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: limit,
    navigation_control_preference: mode,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function projectPropertiesPayload(projectId: string, sortOrder: number): Record<string, unknown> {
  return {
    id: "623e4567-e89b-12d3-a456-426614174005",
    workspace: WS_ID,
    project: projectId,
    user: USER_ID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    preferences: {},
    sort_order: sortOrder,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

interface StubOptions {
  limit?: number;
  mode?: string;
}

function stubClient(options: StubOptions = {}) {
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/user-properties/") && (init.method ?? "GET") === "GET") {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(workspacePropertiesPayload(options.limit ?? 0, options.mode ?? "ACCORDION")),
      };
    }
    if ((init.method ?? "GET") === "PATCH") {
      const match = /\/projects\/([^/]+)\/user-properties\//.exec(url);
      const sent = JSON.parse(init.body ?? "{}") as { sort_order?: number };
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(projectPropertiesPayload(match?.[1] ?? PROJECT_A, sent.sort_order ?? 0)),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls };
}

function setup(props: Partial<SidebarProjectsGroupProps> = {}, stub: StubOptions = {}) {
  const { calls } = stubClient(stub);
  const onCreateProject = vi.fn();
  const onNavigate = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SidebarProjectsGroup
        workspaceSlug="acme"
        projects={PROJECTS}
        projectsStatus="success"
        currentPath="/acme/"
        activeProjectId={null}
        canCreateProject
        onCreateProject={onCreateProject}
        onNavigate={onNavigate}
        {...props}
      />
    </QueryClientProvider>
  );
  return { calls, onCreateProject, onNavigate, queryClient, view };
}

function rowNames(): string[] {
  return screen.getAllByTestId("project-row-link").map((link) => link.textContent ?? "");
}

function pointerMove(clientY: number): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointermove", { clientY, bubbles: true }));
  });
}

function pointerUp(): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointerup", { bubbles: true }));
  });
}

function pointerCancel(): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointercancel", { bubbles: true }));
  });
}

function blurWindow(): void {
  act(() => {
    window.dispatchEvent(new window.FocusEvent("blur"));
  });
}

function stubRowRects(spans: Record<string, { top: number; bottom: number }>): void {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const id = this.dataset["projectRow"];
    const span = id === undefined ? undefined : spans[id];
    const top = span?.top ?? 0;
    const bottom = span?.bottom ?? 0;
    return {
      top,
      bottom,
      left: 0,
      right: 0,
      width: 0,
      height: bottom - top,
      x: 0,
      y: top,
      toJSON: () => ({}),
    } as DOMRect;
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("SidebarProjectsGroup rows", () => {
  it("bundles the aggregate row with the joined projects", () => {
    setup({
      projects: [
        ...PROJECTS,
        project("outsider-id", "Outsider", { member_role: null }),
        project("archived-id", "Archived", { archived_at: "2024-03-04T00:00:00Z" }),
      ],
    });
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute(
      "href",
      "/acme/workspace-views/all-issues/"
    );
    expect(rowNames()).toEqual(["Atlas", "Beacon"]);
  });

  it("caps the listed rows while zero means uncapped", async () => {
    const capped = setup({}, { limit: 1 });
    await waitFor(() => expect(rowNames()).toEqual(["Atlas"]));
    capped.view.unmount();
    const uncapped = setup({}, { limit: 0 });
    await waitFor(() => expect(rowNames()).toEqual(["Atlas", "Beacon"]));
    uncapped.view.unmount();
  });

  it("hides the row toggles in tabbed mode", async () => {
    setup({}, { mode: "TABBED" });
    await waitFor(() => expect(screen.queryByTestId("project-row-toggle")).not.toBeInTheDocument());
    expect(screen.getAllByTestId("project-row-link")).toHaveLength(2);
  });

  it("marks the aggregate row current on workspace views", () => {
    const h = setup({ currentPath: "/acme/workspace-views/all-issues/" });
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <SidebarProjectsGroup
          workspaceSlug="acme"
          projects={PROJECTS}
          projectsStatus="success"
          currentPath="/acme/"
          activeProjectId={null}
          canCreateProject
          onCreateProject={h.onCreateProject}
          onNavigate={h.onNavigate}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("link", { name: "Work Items" })).not.toHaveAttribute("aria-current");
  });
});

describe("SidebarProjectsGroup disclosure", () => {
  it("collapses and reopens through the toggle", () => {
    setup();
    expect(screen.getByTestId("sidebar-projects-list")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Collapse projects group" }));
    expect(screen.queryByTestId("sidebar-projects-list")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand projects group" }));
    expect(screen.getByTestId("sidebar-projects-list")).toBeInTheDocument();
  });

  it("reopens on project, drafts and views routes", () => {
    const h = setup();
    fireEvent.click(screen.getByRole("button", { name: "Collapse projects group" }));
    for (const path of ["/acme/projects/p1/issues", "/acme/drafts/", "/acme/workspace-views/all-issues/"]) {
      h.view.rerender(
        <QueryClientProvider client={h.queryClient}>
          <SidebarProjectsGroup
            workspaceSlug="acme"
            projects={PROJECTS}
            projectsStatus="success"
            currentPath={path}
            activeProjectId={null}
            canCreateProject
            onCreateProject={h.onCreateProject}
            onNavigate={h.onNavigate}
          />
        </QueryClientProvider>
      );
      expect(screen.getByTestId("sidebar-projects-list")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Collapse projects group" }));
    }
  });

  it("stays collapsed across unrelated navigation", () => {
    const h = setup({ currentPath: "/acme/" });
    fireEvent.click(screen.getByRole("button", { name: "Collapse projects group" }));
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <SidebarProjectsGroup
          workspaceSlug="acme"
          projects={PROJECTS}
          projectsStatus="success"
          currentPath="/acme/settings"
          activeProjectId={null}
          canCreateProject
          onCreateProject={h.onCreateProject}
          onNavigate={h.onNavigate}
        />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("sidebar-projects-list")).not.toBeInTheDocument();
  });

  it("stays open without a reported path", () => {
    setup({ currentPath: null });
    expect(screen.getByTestId("sidebar-projects-list")).toBeInTheDocument();
  });
});

describe("SidebarProjectsGroup loading and empty states", () => {
  it("shows four placeholders while the collection loads", () => {
    setup({ projects: [], projectsStatus: "pending" });
    const status = screen.getByRole("status", { name: "Loading projects" });
    expect(status.children).toHaveLength(4);
    expect(screen.queryByTestId("sidebar-projects-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sidebar-empty-create-project")).not.toBeInTheDocument();
  });

  it("offers creation for empty member workspaces", () => {
    const h = setup({ projects: [], projectsStatus: "success" });
    expect(screen.getByRole("link", { name: "Work Items" })).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("sidebar-empty-create-project"));
    expect(h.onCreateProject).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Collapse projects group" }));
    expect(screen.queryByTestId("sidebar-empty-create-project")).not.toBeInTheDocument();
  });

  it("offers creation from the header for members", () => {
    const h = setup();
    fireEvent.click(screen.getByTestId("sidebar-create-project"));
    expect(h.onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("shows guests no creation affordance", () => {
    setup({ projects: [], projectsStatus: "success", canCreateProject: false });
    expect(screen.queryByTestId("sidebar-create-project")).not.toBeInTheDocument();
    expect(screen.queryByTestId("sidebar-empty-create-project")).not.toBeInTheDocument();
  });
});

describe("SidebarProjectsGroup drag reorder", () => {
  const SPANS = {
    [PROJECT_A]: { top: 0, bottom: 30 },
    [PROJECT_B]: { top: 30, bottom: 60 },
  };

  it("drags a row above its target and persists the order", async () => {
    stubRowRects(SPANS);
    const h = setup();
    const handles = screen.getAllByTestId("project-drag-handle");
    fireEvent.pointerDown(handles[1]!);
    pointerMove(5);
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
    pointerUp();
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/projects/${PROJECT_B}/user-properties/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sort_order: 0 });
    expect(screen.queryByTestId("project-drop-indicator")).not.toBeInTheDocument();
  });

  it("lights the indicator below the last row", () => {
    stubRowRects(SPANS);
    setup();
    const handles = screen.getAllByTestId("project-drag-handle");
    fireEvent.pointerDown(handles[0]!);
    pointerMove(500);
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
    pointerUp();
  });

  it("drops nothing without a target", () => {
    stubRowRects(SPANS);
    const h = setup();
    const handles = screen.getAllByTestId("project-drag-handle");
    fireEvent.pointerDown(handles[0]!);
    pointerMove(5);
    expect(screen.queryByTestId("project-drop-indicator")).not.toBeInTheDocument();
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("drops nothing when the order is unchanged", () => {
    stubRowRects(SPANS);
    const h = setup();
    const handles = screen.getAllByTestId("project-drag-handle");
    fireEvent.pointerDown(handles[0]!);
    pointerMove(40);
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(screen.queryByTestId("project-drop-indicator")).not.toBeInTheDocument();
  });

  it("cancels the drag on pointer cancel and on blur", () => {
    stubRowRects(SPANS);
    const h = setup();
    const handles = screen.getAllByTestId("project-drag-handle");
    fireEvent.pointerDown(handles[1]!);
    pointerMove(5);
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
    pointerCancel();
    expect(screen.queryByTestId("project-drop-indicator")).not.toBeInTheDocument();
    fireEvent.pointerDown(handles[1]!);
    pointerMove(5);
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
    blurWindow();
    expect(screen.queryByTestId("project-drop-indicator")).not.toBeInTheDocument();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
  });
});

describe("SidebarProjectsGroup overflow toggle (SHELL-052)", () => {
  it("shows the toggle once the cap leaves rows out", async () => {
    setup({}, { limit: 1 });
    await waitFor(() => expect(rowNames()).toEqual(["Atlas"]));
    expect(screen.getByTestId("projects-overflow-toggle")).toHaveAttribute("aria-expanded", "false");
  });

  it("hides the toggle while everything fits or loads", async () => {
    const fitted = setup({}, { limit: 10 });
    await waitFor(() => expect(rowNames()).toEqual(["Atlas", "Beacon"]));
    expect(screen.queryByTestId("projects-overflow-toggle")).not.toBeInTheDocument();
    fitted.view.unmount();
    setup({ projectsStatus: "pending" }, { limit: 1 });
    expect(screen.queryByTestId("projects-overflow-toggle")).not.toBeInTheDocument();
  });

  it("hides the toggle with the collapsed group", async () => {
    setup({}, { limit: 1 });
    await waitFor(() => expect(rowNames()).toEqual(["Atlas"]));
    fireEvent.click(screen.getByTestId("projects-group-toggle"));
    expect(screen.queryByTestId("projects-overflow-toggle")).not.toBeInTheDocument();
  });

  it("opens and closes the overflow panel", async () => {
    setup({}, { limit: 1 });
    await waitFor(() => expect(rowNames()).toEqual(["Atlas"]));
    fireEvent.click(screen.getByTestId("projects-overflow-toggle"));
    expect(screen.getByTestId("projects-overflow-toggle")).toHaveAttribute("aria-expanded", "true");
    // The panel rides its own chunk (size gate): the toggle flips at once
    // and the slide-over arrives one microtask later.
    expect(await screen.findByTestId("projects-overflow-panel")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("projects-overflow-toggle"));
    expect(screen.queryByTestId("projects-overflow-panel")).not.toBeInTheDocument();
  });
});
