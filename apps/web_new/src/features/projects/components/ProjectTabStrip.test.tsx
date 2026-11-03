// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const P1 = "111e4567-e89b-12d3-a456-426614174001";
const P2 = "222e4567-e89b-12d3-a456-426614174002";
const P3 = "333e4567-e89b-12d3-a456-426614174003";
const UUID = "123e4567-e89b-12d3-a456-426614174000";

let mockPathname = `/acme/projects/${P1}/issues`;
const mockNavigate = vi.fn();

vi.mock("@tanstack/react-router", () => ({
  Link: (props: { to?: string; children?: React.ReactNode; ["aria-current"]?: "page" }) => (
    <a href={props.to} aria-current={props["aria-current"]}>
      {props.children}
    </a>
  ),
  useNavigate: () => mockNavigate,
  useLocation: () => ({ pathname: mockPathname }),
}));

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ProjectTabStrip } from "./ProjectTabStrip.js";

function projectRow(id: string, name: string, memberRole: number | null) {
  return {
    id,
    name,
    identifier: name.toUpperCase(),
    sort_order: 1,
    logo_props: {},
    member_role: memberRole,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

const PROJECTS = [projectRow(P1, "Web", 20), projectRow(P2, "Mobile", 15), projectRow(P3, "Side", null)];

function modePayload(mode: string) {
  return {
    id: UUID,
    workspace: UUID,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: mode,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

function propsPayload(navigation: Record<string, unknown>) {
  return {
    ...modePayload("TABBED"),
    project: P1,
    preferences: { navigation },
    sort_order: 1,
  };
}

type HangTarget = "mode" | "projects" | "props" | "roles" | null;

interface StubOptions {
  mode?: string;
  modeStatus?: number;
  projectsStatus?: number;
  propsStatus?: number;
  rolesStatus?: number;
  roles?: Record<string, number>;
  navigation?: Record<string, unknown>;
  projects?: ReturnType<typeof projectRow>[];
  hang?: HangTarget;
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
    const hang = (target: HangTarget): boolean => options.hang === target;
    if (url.includes("/project-roles/")) {
      if (hang("roles")) return new Promise<never>(() => undefined);
      const status = options.rolesStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(options.roles ?? { [P1]: 20, [P2]: 15 }) : "{}"),
      };
    }
    if (url.includes(`/projects/${P1}/user-properties/`)) {
      if (hang("props")) return new Promise<never>(() => undefined);
      if (init.method === "PATCH") {
        return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(propsPayload({})) };
      }
      const status = options.propsStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status === 200
            ? JSON.stringify(propsPayload(options.navigation ?? { default_tab: "cycles", hide_in_more_menu: [] }))
            : "{}",
      };
    }
    if (url.endsWith("/user-properties/")) {
      if (hang("mode")) return new Promise<never>(() => undefined);
      const status = options.modeStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(modePayload(options.mode ?? "TABBED")) : "{}"),
      };
    }
    if (url.endsWith("/projects/")) {
      if (hang("projects")) return new Promise<never>(() => undefined);
      const status = options.projectsStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(options.projects ?? PROJECTS) : "{}"),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

function setup(options: StubOptions = {}, workItem: { id: string; projectId: string; isEpic: boolean } | null = null) {
  const calls = stubClient(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectTabStrip workspaceSlug="acme" projectId={P1} workItem={workItem} />
    </QueryClientProvider>
  );
  return { calls, queryClient };
}

/** Settle signal: every strip query left pending, so absence is final. */
async function waitForSettled(queryClient: QueryClient, settled: number): Promise<void> {
  await waitFor(() => {
    const states = queryClient
      .getQueryCache()
      .getAll()
      .map((query) => query.state.status);
    expect(states.filter((status) => status !== "pending")).toHaveLength(settled);
  });
}

function tabNames(): string[] {
  return screen.queryAllByRole("link").map((link) => link.textContent ?? "");
}

beforeEach(() => {
  vi.clearAllMocks();
  mockPathname = `/acme/projects/${P1}/issues`;
});

afterEach(() => {
  resetAppClient();
});

describe("ProjectTabStrip", () => {
  it("renders the header and the gated tabs with the active one marked", async () => {
    setup();
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByTestId("project-switcher-trigger")).toHaveTextContent("Web");
    expect(tabNames()).toEqual(["Work Items", "Cycles", "Modules", "Views", "Pages", "Intake", "Schedulers"]);
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
  });

  it("lists joined projects only in the switcher", async () => {
    setup();
    await screen.findByTestId("project-tab-strip");
    fireEvent.click(screen.getByTestId("project-switcher-trigger"));
    expect(screen.getByRole("menuitem", { name: "Web" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Mobile" })).toBeInTheDocument();
    expect(screen.getAllByRole("menuitem")).toHaveLength(2);
  });

  it("stays hidden outside tabbed mode", async () => {
    const { queryClient } = setup({ mode: "ACCORDION" });
    await waitForSettled(queryClient, 4);
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("stays hidden when the mode read fails", async () => {
    const { queryClient } = setup({ modeStatus: 500 });
    await waitForSettled(queryClient, 4);
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("stays hidden when the projects read fails", async () => {
    const { queryClient } = setup({ projectsStatus: 500 });
    await waitForSettled(queryClient, 4);
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("stays hidden when the standing read fails", async () => {
    const { queryClient } = setup({ rolesStatus: 500 });
    await waitForSettled(queryClient, 4);
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("stays hidden without current project data", async () => {
    const { queryClient } = setup({ projects: [projectRow(P2, "Mobile", 15)] });
    await waitForSettled(queryClient, 4);
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("reads guest-level tabs without a project role", async () => {
    setup({ roles: {} });
    await screen.findByTestId("project-tab-strip");
    expect(tabNames()).toEqual(["Work Items", "Views", "Pages", "Intake", "Schedulers"]);
  });

  it("falls back to default preferences when the props read fails", async () => {
    setup({ propsStatus: 500 });
    await screen.findByTestId("project-tab-strip");
    // Tracking default: Cycles offers to become the default.
    fireEvent.contextMenu(screen.getByRole("link", { name: "Cycles" }));
    expect(screen.getByRole("menuitem", { name: "Set as default" })).toBeInTheDocument();
  });

  it.each(["mode", "projects", "props", "roles"] as const)("waits while %s loads", async (hang) => {
    const { queryClient } = setup({ hang });
    await waitForSettled(queryClient, 3);
    expect(screen.queryByTestId("project-tab-strip")).not.toBeInTheDocument();
  });

  it("opens the picked project at the stored default tab", async () => {
    setup();
    await screen.findByTestId("project-tab-strip");
    fireEvent.click(screen.getByTestId("project-switcher-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Mobile" }));
    expect(mockNavigate).toHaveBeenCalledWith({ to: `/acme/projects/${P2}/cycles` });
  });

  it("highlights tracking for a work item of this project", async () => {
    mockPathname = "/acme/browse/WEB-1";
    setup({}, { id: "w1", projectId: P1, isEpic: false });
    await screen.findByTestId("project-tab-strip");
    expect(screen.getByRole("link", { name: "Work Items" })).toHaveAttribute("aria-current", "page");
  });

  it("highlights nothing for an epic while no epic tab renders", async () => {
    mockPathname = "/acme/browse/WEB-1";
    setup({}, { id: "w1", projectId: P1, isEpic: true });
    await screen.findByTestId("project-tab-strip");
    expect(screen.queryByRole("link", { current: "page" })).not.toBeInTheDocument();
  });

  it("persists default choices through the preferences mutation", async () => {
    const { calls } = setup({ navigation: { default_tab: "work_items", hide_in_more_menu: [] } });
    await screen.findByTestId("project-tab-strip");
    fireEvent.contextMenu(screen.getByRole("link", { name: "Cycles" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Set as default" }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}").preferences.navigation.default_tab).toBe("cycles");
  });
});
