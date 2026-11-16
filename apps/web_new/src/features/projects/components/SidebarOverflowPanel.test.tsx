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
import { SIDEBAR_DEFAULT_WIDTH, useShellChrome } from "../../../shared/shell/chromeState.js";
import { SidebarOverflowPanel, type SidebarOverflowPanelProps } from "./SidebarOverflowPanel.js";
import {
  ROW_COPY_FAILED_BODY,
  ROW_COPY_FAILED_TITLE,
  ROW_LINK_COPIED_BODY,
  ROW_LINK_COPIED_TITLE,
} from "./SidebarProjectRow.js";

// The runners sub-nav entry renders as a router Link (migrated route), so
// stand it in as a plain anchor and assert the navigation intent only.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/toaster.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_A = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_B = "323e4567-e89b-12d3-a456-426614174002";

function project(id: string, name: string, overrides: Partial<Project> = {}): Project {
  return {
    id,
    name,
    identifier: "PAR",
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
  project(PROJECT_A, "Atlas", { identifier: "ATL", sort_order: 10000 }),
  project(PROJECT_B, "Beacon", { identifier: "BCN", sort_order: 20000 }),
];

const CLIPBOARD_DESCRIPTOR = Object.getOwnPropertyDescriptor(globalThis.navigator, "clipboard");
const assign = vi.fn();

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
}

function stubClient() {
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
    if ((init.method ?? "GET") === "PATCH") {
      const match = /\/projects\/([^/]+)\/user-properties\//.exec(url);
      const sent = JSON.parse(init.body ?? "{}") as { sort_order?: number };
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            id: "723e4567-e89b-12d3-a456-426614174006",
            workspace: WS_ID,
            project: match?.[1] ?? PROJECT_A,
            user: "823e4567-e89b-12d3-a456-426614174007",
            filters: {},
            display_filters: {},
            display_properties: {},
            rich_filters: {},
            preferences: {},
            sort_order: sent.sort_order ?? 0,
            created_by: null,
            updated_by: null,
            created_at: "2024-01-02T03:04:05Z",
            updated_at: "2024-02-03T04:05:06Z",
          }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls };
}

function setup(props: Partial<SidebarOverflowPanelProps> = {}) {
  const { calls } = stubClient();
  const onCreateProject = vi.fn();
  const onNavigate = vi.fn();
  const onClose = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <SidebarOverflowPanel
        workspaceSlug="acme"
        projects={PROJECTS}
        canCreateProject
        tabbed={false}
        currentPath="/acme/"
        activeProjectId={null}
        onCreateProject={onCreateProject}
        onNavigate={onNavigate}
        onClose={onClose}
        {...props}
      />
    </QueryClientProvider>
  );
  return { calls, onCreateProject, onNavigate, onClose, view };
}

function panelNames(): string[] {
  return screen.queryAllByTestId("project-row-link").map((link) => link.textContent ?? "");
}

function menuNames(): string[] {
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

function openMenu(name: string): void {
  fireEvent.click(screen.getByRole("button", { name: `Actions for ${name}` }));
}

function escapeOn(target: Element): void {
  act(() => {
    target.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
}

function stubRowRects(spans: Record<string, { top: number; bottom: number }>): void {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const id = this.dataset["overflowRow"];
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

beforeEach(() => {
  window.localStorage.clear();
  vi.clearAllMocks();
  Object.defineProperty(globalThis.window, "location", {
    value: { assign, origin: "https://app.test" },
    writable: true,
    configurable: true,
  });
  act(() => {
    useShellChrome.getState().setSidebarCollapsed(false);
    useShellChrome.getState().setSidebarWidth(SIDEBAR_DEFAULT_WIDTH);
  });
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  act(() => {
    useShellChrome.getState().setSidebarCollapsed(false);
    useShellChrome.getState().setSidebarWidth(SIDEBAR_DEFAULT_WIDTH);
  });
  if (CLIPBOARD_DESCRIPTOR === undefined) {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
  } else {
    Object.defineProperty(globalThis.navigator, "clipboard", CLIPBOARD_DESCRIPTOR);
  }
});

describe("SidebarOverflowPanel", () => {
  it("lists every joined project with tracking links", () => {
    setup();
    expect(panelNames()).toEqual(["ATLAtlas", "BCNBeacon"]);
    const links = screen.getByTestId("projects-overflow-list").querySelectorAll("a");
    expect(links[0]?.getAttribute("href")).toBe(`/acme/projects/${PROJECT_A}/issues`);
  });

  it("follows plain left clicks client-side", () => {
    const h = setup();
    const link = screen.getAllByTestId("project-row-link")[0];
    if (link === undefined) throw new Error("missing panel row");
    fireEvent.click(link, { button: 0 });
    expect(h.onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_A}/issues`);
    fireEvent.click(link, { button: 0, ctrlKey: true });
    expect(h.onNavigate).toHaveBeenCalledTimes(1);
  });

  it("filters by name or short code and empties hopelessly", () => {
    setup();
    const search = screen.getByTestId("projects-overflow-search");
    fireEvent.change(search, { target: { value: "Beacon" } });
    expect(panelNames()).toEqual(["BCNBeacon"]);
    fireEvent.change(search, { target: { value: "ATL" } });
    expect(panelNames()).toEqual(["ATLAtlas"]);
    fireEvent.change(search, { target: { value: "zzz-no-such-project" } });
    expect(panelNames()).toEqual([]);
    expect(screen.getByTestId("projects-overflow-empty")).toBeInTheDocument();
    fireEvent.change(search, { target: { value: "" } });
    expect(panelNames()).toEqual(["ATLAtlas", "BCNBeacon"]);
  });

  it("offers creation to admins and members only", () => {
    const h = setup();
    expect(screen.getByTestId("projects-overflow-create")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("projects-overflow-create"));
    expect(h.onCreateProject).toHaveBeenCalledTimes(1);
    h.view.unmount();
    setup({ canCreateProject: false });
    expect(screen.queryByTestId("projects-overflow-create")).not.toBeInTheDocument();
  });

  it("copies row links from the menu with the shared confirmation", async () => {
    const written: string[] = [];
    stubClipboard(async (text: string) => void written.push(text));
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(ROW_LINK_COPIED_TITLE, ROW_LINK_COPIED_BODY));
    expect(written).toEqual([`https://app.test/acme/projects/${PROJECT_A}/issues`]);
  });

  it("toasts when the clipboard refuses or is missing", async () => {
    stubClipboard(async () => {
      throw new Error("denied");
    });
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(ROW_COPY_FAILED_TITLE, ROW_COPY_FAILED_BODY));
  });

  it("toasts when the clipboard is unavailable", async () => {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
    setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(ROW_COPY_FAILED_TITLE, ROW_COPY_FAILED_BODY));
  });

  it("closes on outside clicks but not inside ones or the toggle", () => {
    const h = setup();
    const { unmount } = render(<button type="button" data-testid="projects-overflow-toggle" />);
    act(() => {
      screen
        .getByTestId("projects-overflow-panel")
        .dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    });
    expect(h.onClose).not.toHaveBeenCalled();
    act(() => {
      screen
        .getByTestId("projects-overflow-toggle")
        .dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    });
    expect(h.onClose).not.toHaveBeenCalled();
    act(() => {
      document.body.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    });
    expect(h.onClose).toHaveBeenCalledTimes(1);
    unmount();
  });

  it("closes on Escape only", () => {
    const h = setup();
    act(() => {
      document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "a", bubbles: true }));
    });
    expect(h.onClose).not.toHaveBeenCalled();
    act(() => {
      document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(h.onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the sidebar collapses and sits beside it", () => {
    const h = setup();
    expect(screen.getByTestId("projects-overflow-panel")).toHaveStyle({ left: `${SIDEBAR_DEFAULT_WIDTH}px` });
    act(() => {
      useShellChrome.getState().setSidebarCollapsed(true);
    });
    expect(h.onClose).toHaveBeenCalledTimes(1);
  });

  it("reorders rows with one position write", async () => {
    const h = setup();
    stubRowRects({ [PROJECT_A]: { top: 0, bottom: 20 }, [PROJECT_B]: { top: 20, bottom: 40 } });
    const handles = screen.getAllByTestId("overflow-drag-handle");
    const handle = handles[1];
    if (!(handle instanceof HTMLElement)) throw new Error("missing drag handle");
    fireEvent.pointerDown(handle);
    pointerMove(5);
    await waitFor(() => expect(screen.getAllByTestId("overflow-drop-indicator")).toHaveLength(1));
    pointerUp();
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/projects/${PROJECT_B}/user-properties/`);
  });

  it("stays silent on cancelled, self and no-op drops", () => {
    const h = setup();
    stubRowRects({ [PROJECT_A]: { top: 0, bottom: 20 }, [PROJECT_B]: { top: 20, bottom: 40 } });
    const handles = screen.getAllByTestId("overflow-drag-handle");
    const first = handles[0];
    if (!(first instanceof HTMLElement)) throw new Error("missing drag handle");
    fireEvent.pointerDown(first);
    pointerMove(200);
    pointerCancel();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    fireEvent.pointerDown(first);
    pointerMove(5);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    fireEvent.pointerDown(first);
    pointerMove(25);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("marks below-last drops", async () => {
    setup();
    stubRowRects({ [PROJECT_A]: { top: 0, bottom: 20 }, [PROJECT_B]: { top: 20, bottom: 40 } });
    const first = screen.getAllByTestId("overflow-drag-handle")[0];
    if (!(first instanceof HTMLElement)) throw new Error("missing drag handle");
    fireEvent.pointerDown(first);
    pointerMove(200);
    await waitFor(() => expect(screen.getAllByTestId("overflow-drop-indicator")).toHaveLength(1));
    pointerCancel();
  });
});

describe("SidebarOverflowPanel quick menu", () => {
  it("offers admins publish, copy, archives and settings", () => {
    setup();
    openMenu("Atlas");
    expect(menuNames()).toEqual(["Publish project", "Copy link", "Archives", "Settings"]);
  });

  it("offers members copy, archives and settings", () => {
    setup({ projects: [project(PROJECT_A, "Atlas", { member_role: 15 })] });
    openMenu("Atlas");
    expect(menuNames()).toEqual(["Copy link", "Archives", "Settings"]);
  });

  it("offers project guests copy, settings and leave", () => {
    setup({ projects: [project(PROJECT_A, "Atlas", { member_role: 5 })] });
    openMenu("Atlas");
    expect(menuNames()).toEqual(["Copy link", "Settings", "Leave project"]);
  });

  it("opens the publish dialog without closing the panel", async () => {
    const h = setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Publish project" }));
    expect(await screen.findByRole("heading", { name: "Publish project" })).toBeInTheDocument();
    expect(h.onClose).not.toHaveBeenCalled();
  });

  it("opens the leave dialog without closing the panel", async () => {
    const h = setup({ projects: [project(PROJECT_A, "Atlas", { member_role: 5 })] });
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Leave project" }));
    expect(await screen.findByRole("heading", { name: "Leave project" })).toBeInTheDocument();
    expect(h.onClose).not.toHaveBeenCalled();
  });

  it("loads archives and settings outside the client router and closes the panel", () => {
    const h = setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Archives" }));
    expect(assign).toHaveBeenCalledWith(`/acme/projects/${PROJECT_A}/archives/issues`);
    expect(h.onClose).toHaveBeenCalledTimes(1);
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(assign).toHaveBeenCalledWith(`/acme/settings/projects/${PROJECT_A}`);
    expect(h.onClose).toHaveBeenCalledTimes(2);
  });

  it("exposes the toggle and the menu to the keyboard with row names", () => {
    setup();
    const toggle = screen.getByRole("button", { name: "Expand Atlas" });
    const menu = screen.getByRole("button", { name: "Actions for Atlas" });
    expect(toggle.tabIndex).toBeGreaterThanOrEqual(0);
    expect(menu.tabIndex).toBeGreaterThanOrEqual(0);
  });
});

describe("SidebarOverflowPanel expansion", () => {
  it("expands and collapses rows through the toggle", () => {
    setup();
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    expect(screen.getByTestId("project-subnav")).toHaveAccessibleName("Atlas sections");
    expect(screen.getByRole("button", { name: "Collapse Atlas" })).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(screen.getByRole("button", { name: "Collapse Atlas" }));
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
  });

  it("hides the toggle in tabbed mode", () => {
    setup({ tabbed: true });
    expect(screen.queryByTestId("project-row-toggle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
  });

  it("starts the route project expanded", () => {
    setup({ activeProjectId: PROJECT_A });
    expect(screen.getByTestId("project-subnav")).toHaveAccessibleName("Atlas sections");
  });

  it("expands the route project on arrival", () => {
    const h = setup();
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
    h.view.rerender(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <SidebarOverflowPanel
          workspaceSlug="acme"
          projects={PROJECTS}
          canCreateProject
          tabbed={false}
          currentPath="/acme/"
          activeProjectId={PROJECT_B}
          onCreateProject={() => undefined}
          onNavigate={() => undefined}
          onClose={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-subnav")).toHaveAccessibleName("Beacon sections");
  });

  it("lists flag-gated entries with slashed addresses", () => {
    setup({
      projects: [project(PROJECT_A, "Atlas", { page_view: true, issue_views_view: true })],
    });
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    const links = screen.getAllByTestId("project-subnav-link");
    expect(links.map((link) => link.textContent)).toEqual(
      expect.arrayContaining(["Work Items", "Views", "Pages", "Schedulers", "AI Workers"])
    );
    expect(links.map((link) => link.textContent)).not.toContain("Cycles");
    expect(links.map((link) => link.textContent)).not.toContain("Modules");
    for (const link of links) {
      expect(link.getAttribute("href")).toContain(`/projects/${PROJECT_A}/`);
    }
  });

  it("hides member-plus entries from project guests", () => {
    setup({ projects: [project(PROJECT_A, "Atlas", { member_role: 5 })] });
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    const texts = screen.getAllByTestId("project-subnav-link").map((link) => link.textContent);
    expect(texts).toEqual(expect.arrayContaining(["Work Items", "Schedulers"]));
    expect(texts).not.toContain("AI Workers");
  });

  it("marks the current entry and badges pending intake", () => {
    setup({
      projects: [project(PROJECT_A, "Atlas", { page_view: true, inbox_view: true, intake_count: 4 })],
      currentPath: `/acme/projects/${PROJECT_A}/pages/`,
      activeProjectId: PROJECT_A,
    });
    const entries = screen.getAllByTestId("project-subnav-link");
    const pages = entries.find((link) => (link.textContent ?? "").includes("Pages"));
    expect(pages).toHaveAttribute("aria-current", "page");
    const intake = entries.find((link) => (link.textContent ?? "").includes("Intake"));
    expect(intake?.textContent).toContain("4");
  });

  it("navigates from section links and closes the panel", () => {
    const h = setup({ activeProjectId: PROJECT_A });
    const entries = screen.getAllByTestId("project-subnav-link");
    const schedulers = entries.find((link) => (link.textContent ?? "").includes("Schedulers"));
    if (schedulers === undefined) throw new Error("schedulers entry missing");
    fireEvent.click(schedulers);
    expect(h.onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_A}/schedulers/`);
    expect(h.onClose).toHaveBeenCalledTimes(1);
  });

  it("leaves modified section clicks to the browser", () => {
    const h = setup({ activeProjectId: PROJECT_A });
    const entries = screen.getAllByTestId("project-subnav-link");
    const schedulers = entries.find((link) => (link.textContent ?? "").includes("Schedulers"));
    if (schedulers === undefined) throw new Error("schedulers entry missing");
    fireEvent.click(schedulers, { metaKey: true, button: 0 });
    expect(h.onNavigate).not.toHaveBeenCalled();
    expect(h.onClose).not.toHaveBeenCalled();
  });

  it("closes the panel from the runners entry without double navigation", () => {
    const h = setup({ activeProjectId: PROJECT_A });
    const entries = screen.getAllByTestId("project-subnav-link");
    const runners = entries.find((link) => (link.textContent ?? "").includes("AI Workers"));
    if (runners === undefined) throw new Error("runners entry missing");
    fireEvent.click(runners);
    expect(h.onNavigate).not.toHaveBeenCalled();
    expect(h.onClose).toHaveBeenCalledTimes(1);
  });

  it("leaves modified runners clicks to the browser", () => {
    const h = setup({ activeProjectId: PROJECT_A });
    const entries = screen.getAllByTestId("project-subnav-link");
    const runners = entries.find((link) => (link.textContent ?? "").includes("AI Workers"));
    if (runners === undefined) throw new Error("runners entry missing");
    fireEvent.click(runners, { metaKey: true, button: 0 });
    expect(h.onNavigate).not.toHaveBeenCalled();
    expect(h.onClose).not.toHaveBeenCalled();
  });

  it("keeps expanded rows expanded across search typing", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    expect(screen.getByTestId("project-subnav")).toBeInTheDocument();
    const search = screen.getByTestId("projects-overflow-search");
    fireEvent.change(search, { target: { value: "Beacon" } });
    expect(panelNames()).toEqual(["BCNBeacon"]);
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "" } });
    expect(panelNames()).toEqual(["ATLAtlas", "BCNBeacon"]);
    expect(screen.getByTestId("project-subnav")).toHaveAccessibleName("Atlas sections");
  });

  it("collapses sub-navs while dragging and restores them after", async () => {
    setup();
    stubRowRects({ [PROJECT_A]: { top: 0, bottom: 20 }, [PROJECT_B]: { top: 20, bottom: 40 } });
    fireEvent.click(screen.getByRole("button", { name: "Expand Atlas" }));
    expect(screen.getByTestId("project-subnav")).toBeInTheDocument();
    const handle = screen.getAllByTestId("overflow-drag-handle")[0];
    if (!(handle instanceof HTMLElement)) throw new Error("missing drag handle");
    fireEvent.pointerDown(handle);
    expect(screen.queryByTestId("project-subnav")).not.toBeInTheDocument();
    pointerUp();
    await waitFor(() => expect(screen.getByTestId("project-subnav")).toBeInTheDocument());
  });

  it("does not start a drag from the toggle or the menu", () => {
    const h = setup();
    stubRowRects({ [PROJECT_A]: { top: 0, bottom: 20 }, [PROJECT_B]: { top: 20, bottom: 40 } });
    fireEvent.pointerDown(screen.getByRole("button", { name: "Expand Atlas" }));
    pointerMove(200);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Actions for Atlas" }));
    pointerMove(200);
    pointerUp();
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
  });
});

describe("SidebarOverflowPanel overlay interplay", () => {
  it("ignores outside clicks inside an open row menu", () => {
    const h = setup();
    openMenu("Atlas");
    const item = screen.getByRole("menuitem", { name: "Copy link" });
    act(() => {
      item.dispatchEvent(new window.MouseEvent("pointerdown", { bubbles: true }));
    });
    expect(h.onClose).not.toHaveBeenCalled();
  });

  it("closes an open menu before the panel on Escape", () => {
    const h = setup();
    openMenu("Atlas");
    const menu = screen.getByRole("menu", { name: "Actions for Atlas" });
    escapeOn(menu);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(h.onClose).not.toHaveBeenCalled();
    act(() => {
      document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(h.onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the panel open while a row dialog owns Escape", async () => {
    const h = setup();
    openMenu("Atlas");
    fireEvent.click(screen.getByRole("menuitem", { name: "Publish project" }));
    expect(await screen.findByRole("heading", { name: "Publish project" })).toBeInTheDocument();
    const dialogs = screen.getAllByRole("dialog");
    const overlay = dialogs.find((dialog) => dialog.getAttribute("data-testid") !== "projects-overflow-panel");
    if (overlay === undefined) throw new Error("missing dialog overlay");
    escapeOn(overlay);
    expect(h.onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("projects-overflow-panel")).toBeInTheDocument();
  });
});
