// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Project, ProjectUserProperties } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { projectKeys } from "../api/keys.js";
import { ROW_LINK_COPIED_TITLE, SidebarProjectRow, type SidebarProjectRowProps } from "./SidebarProjectRow.js";

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/toaster.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const USER_ID = "323e4567-e89b-12d3-a456-426614174002";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: PROJECT_ID,
    name: "Atlas",
    identifier: "ATL",
    sort_order: 10000,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
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

function properties(preferences: Record<string, unknown>): ProjectUserProperties {
  return {
    id: "423e4567-e89b-12d3-a456-426614174003",
    workspace: WS_ID,
    project: PROJECT_ID,
    user: USER_ID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    preferences,
    sort_order: 10000,
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

const CLIPBOARD_DESCRIPTOR = Object.getOwnPropertyDescriptor(globalThis.navigator, "clipboard");
const assign = vi.fn();

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
}

function setup(props: Partial<SidebarProjectRowProps> = {}) {
  const onNavigate = vi.fn();
  const onDragStart = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SidebarProjectRow
        workspaceSlug="acme"
        project={project()}
        currentPath="/acme/"
        activeProjectId={null}
        tabbed={false}
        onNavigate={onNavigate}
        dropIndicator={null}
        isDragging={false}
        onDragStart={onDragStart}
        {...props}
      />
    </QueryClientProvider>
  );
  return { onNavigate, onDragStart, queryClient, view };
}

function menuNames(): string[] {
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

function openMenu(name: string): void {
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${name}` }));
}

beforeEach(() => {
  vi.clearAllMocks();
  // The row never fetches: the default-tab read observes the warmed
  // cache only, so any transport touch fails the test outright.
  createAppClient("https://api.test", (() => {
    throw new Error("sidebar row must not fetch");
  }) as never);
  Object.defineProperty(globalThis.window, "location", {
    value: { assign, origin: "https://app.test" },
    writable: true,
    configurable: true,
  });
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  if (CLIPBOARD_DESCRIPTOR === undefined) {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
  } else {
    Object.defineProperty(globalThis.navigator, "clipboard", CLIPBOARD_DESCRIPTOR);
  }
  Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
});

describe("SidebarProjectRow links", () => {
  it("links tracking with a cold member store", () => {
    setup();
    expect(screen.getByTestId("project-row-link")).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/issues`);
  });

  it("links the configured default tab once the store warms", async () => {
    const h = setup();
    expect(screen.getByTestId("project-row-link")).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/issues`);
    h.queryClient.setQueryData(projectKeys.projectProperties("acme", PROJECT_ID), () =>
      properties({ navigation: { default_tab: "pages", hide_in_more_menu: [] } })
    );
    await waitFor(() =>
      expect(screen.getByTestId("project-row-link")).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/pages`)
    );
  });

  it("falls back to tracking for unknown stored tabs", () => {
    const h = setup();
    h.queryClient.setQueryData(projectKeys.projectProperties("acme", PROJECT_ID), () =>
      properties({ navigation: { default_tab: "nope", hide_in_more_menu: [] } })
    );
    expect(screen.getByTestId("project-row-link")).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/issues`);
  });
});

describe("SidebarProjectRow expansion", () => {
  it("expands and collapses through the toggle in accordion mode", () => {
    setup();
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    expect(screen.getByTestId("project-subnav")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse Atlas" })).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(screen.getByRole("button", { name: "Collapse Atlas" }));
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
  });

  it("toggles without navigating on row clicks in accordion mode", () => {
    const h = setup();
    fireEvent.click(screen.getByTestId("project-row-link"));
    expect(h.onNavigate).not.toHaveBeenCalled();
    expect(screen.getByTestId("project-subnav")).toBeInTheDocument();
  });

  it("leaves modified clicks to the browser", () => {
    const h = setup();
    fireEvent.click(screen.getByTestId("project-row-link"), { metaKey: true, button: 0 });
    expect(h.onNavigate).not.toHaveBeenCalled();
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
  });

  it("navigates on row clicks in tabbed mode without any toggle", () => {
    const h = setup({ tabbed: true });
    expect(screen.queryByTestId("project-row-toggle")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("project-row-link"));
    expect(h.onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/issues`);
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
  });

  it("highlights the route project in tabbed mode only", () => {
    const { view } = setup({ tabbed: true, activeProjectId: PROJECT_ID });
    expect(screen.getByTestId("project-row-link")).toHaveAttribute("aria-current", "page");
    view.rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SidebarProjectRow
          workspaceSlug="acme"
          project={project()}
          currentPath="/acme/"
          activeProjectId={PROJECT_ID}
          tabbed={false}
          onNavigate={() => undefined}
          dropIndicator={null}
          isDragging={false}
          onDragStart={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-row-link")).not.toHaveAttribute("aria-current");
  });

  it("opens the route project on mount and on arrival", () => {
    const h = setup({ activeProjectId: PROJECT_ID });
    expect(screen.getByTestId("project-subnav")).toBeInTheDocument();
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <SidebarProjectRow
          workspaceSlug="acme"
          project={project()}
          currentPath="/acme/"
          activeProjectId={null}
          tabbed={false}
          onNavigate={h.onNavigate}
          dropIndicator={null}
          isDragging={false}
          onDragStart={h.onDragStart}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Collapse Atlas" }));
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <SidebarProjectRow
          workspaceSlug="acme"
          project={project()}
          currentPath={`/acme/projects/${PROJECT_ID}/issues`}
          activeProjectId={PROJECT_ID}
          tabbed={false}
          onNavigate={h.onNavigate}
          dropIndicator={null}
          isDragging={false}
          onDragStart={h.onDragStart}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-subnav")).toBeInTheDocument();
  });

  it("marks no subnav entry without a reported path", () => {
    setup({ currentPath: null, activeProjectId: PROJECT_ID });
    const subnav = screen.getByTestId("project-subnav");
    expect(subnav.querySelector('[aria-current="page"]')).toBeNull();
  });

  it("scrolls the route project into view on mount", () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { value: scrollIntoView, configurable: true });
    setup({ activeProjectId: PROJECT_ID });
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
  });

  it("stays put off the route project", () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { value: scrollIntoView, configurable: true });
    setup();
    expect(scrollIntoView).not.toHaveBeenCalled();
  });
});

describe("SidebarProjectRow sub-navigation", () => {
  it("lists flag-gated entries with slashed addresses", () => {
    setup({ project: project({ cycle_view: false, module_view: false, issue_views_view: false, page_view: true }) });
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    const links = screen.getAllByTestId("project-subnav-link");
    expect(links.map((link) => link.textContent)).toEqual(
      expect.arrayContaining(["Work Items", "Pages", "Schedulers", "AI Workers"])
    );
    expect(links.map((link) => link.textContent)).not.toContain("Cycles");
    expect(links.map((link) => link.textContent)).not.toContain("Modules");
    expect(links.map((link) => link.textContent)).not.toContain("Views");
    for (const link of links) {
      expect(link.getAttribute("href")).toContain(`/projects/${PROJECT_ID}/`);
    }
  });

  it("hides member-plus entries from project guests", () => {
    setup({ project: project({ member_role: 5 }) });
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    const texts = screen.getAllByTestId("project-subnav-link").map((link) => link.textContent);
    expect(texts).toEqual(expect.arrayContaining(["Work Items", "Pages", "Schedulers"]));
    expect(texts).not.toContain("Cycles");
    expect(texts).not.toContain("Modules");
    expect(texts).not.toContain("AI Workers");
  });

  it("marks the current entry and badges pending intake", () => {
    setup({
      project: project({ inbox_view: true, intake_count: 4 }),
      currentPath: `/acme/projects/${PROJECT_ID}/pages/`,
      activeProjectId: PROJECT_ID,
    });
    const entries = screen.getAllByTestId("project-subnav-link");
    const current = entries.find((link) => (link.textContent ?? "").includes("Pages"));
    expect(current).toHaveAttribute("aria-current", "page");
    const intake = entries.find((link) => (link.textContent ?? "").includes("Intake"));
    expect(intake?.textContent).toContain("4");
  });

  it("navigates client-side from entries", () => {
    const h = setup({ activeProjectId: PROJECT_ID });
    const entries = screen.getAllByTestId("project-subnav-link");
    const pages = entries.find((link) => (link.textContent ?? "").includes("Pages"));
    if (pages === undefined) throw new Error("pages entry missing");
    fireEvent.click(pages);
    expect(h.onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/pages/`);
  });

  it("leaves modified entry clicks to the browser", () => {
    const h = setup({ activeProjectId: PROJECT_ID });
    const entries = screen.getAllByTestId("project-subnav-link");
    const pages = entries.find((link) => (link.textContent ?? "").includes("Pages"));
    if (pages === undefined) throw new Error("pages entry missing");
    fireEvent.click(pages, { metaKey: true, button: 0 });
    expect(h.onNavigate).not.toHaveBeenCalled();
  });
});

describe("SidebarProjectRow quick menu", () => {
  it("offers admins publish, copy, archives and settings", () => {
    setup();
    openMenu("Atlas");
    expect(menuNames()).toEqual(["Publish project", "Copy link", "Archives", "Settings"]);
  });

  it("offers members copy, archives and settings", () => {
    setup({ project: project({ member_role: 15 }) });
    openMenu("Atlas");
    expect(menuNames()).toEqual(["Copy link", "Archives", "Settings"]);
  });

  it("offers project guests copy, settings and leave", () => {
    setup({ project: project({ member_role: 5 }) });
    openMenu("Atlas");
    expect(menuNames()).toEqual(["Copy link", "Settings", "Leave project"]);
  });

  it("opens the publish dialog from its entry", async () => {
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Publish project" }));
    expect(await screen.findByRole("heading", { name: "Publish project" })).toBeInTheDocument();
  });

  it("opens the leave dialog from its entry", async () => {
    setup({ project: project({ member_role: 5 }) });
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Leave project" }));
    expect(await screen.findByRole("heading", { name: "Leave project" })).toBeInTheDocument();
  });

  it("copies the tracking address with confirmation", async () => {
    const writeText = vi.fn(async () => undefined);
    stubClipboard(writeText);
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(`https://app.test/acme/projects/${PROJECT_ID}/issues`));
    expect(notifySuccess).toHaveBeenCalledWith(ROW_LINK_COPIED_TITLE, expect.any(String));
  });

  it("toasts when the clipboard is missing or refuses", async () => {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    stubClipboard(async () => {
      throw new Error("denied");
    });
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledTimes(2));
  });

  it("loads archives and settings outside the client router", () => {
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Archives" }));
    expect(assign).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/archives/issues`);
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(assign).toHaveBeenCalledWith(`/acme/settings/projects/${PROJECT_ID}`);
  });
});

describe("SidebarProjectRow drag handle", () => {
  it("starts the drag from the handle", () => {
    const h = setup();
    const handle = screen.getByTestId("project-drag-handle");
    const event = fireEvent.pointerDown(handle);
    expect(event).toBe(false);
    expect(h.onDragStart).toHaveBeenCalledWith(PROJECT_ID);
  });

  it("offers no handle without a stored position", () => {
    const h = setup({ project: project({ sort_order: null }) });
    expect(screen.queryByTestId("project-drag-handle")).not.toBeInTheDocument();
    expect(h.onDragStart).not.toHaveBeenCalled();
  });

  it("lights the drop indicator above or below", () => {
    const h = setup({ dropIndicator: "above" });
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
    h.view.rerender(
      <QueryClientProvider client={h.queryClient}>
        <SidebarProjectRow
          workspaceSlug="acme"
          project={project()}
          currentPath="/acme/"
          activeProjectId={null}
          tabbed={false}
          onNavigate={h.onNavigate}
          dropIndicator="below"
          isDragging
          onDragStart={h.onDragStart}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-drop-indicator")).toBeInTheDocument();
  });
});
