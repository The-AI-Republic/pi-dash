// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics screen tests (parity/specs/analytics.md §§1, 3–5, 8, ANL-001–005, ANL-008).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the screen composition.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: {
    to?: string;
    search?: Record<string, unknown>;
    children?: React.ReactNode;
    role?: string;
    id?: string;
    ["aria-selected"]?: boolean;
    ["aria-controls"]?: string;
    ["data-tab-id"]?: string;
    ["data-testid"]?: string;
  }) => (
    <a
      href={props.to}
      data-search={JSON.stringify(props.search ?? {})}
      role={props.role}
      id={props.id}
      aria-selected={props["aria-selected"]}
      aria-controls={props["aria-controls"]}
      data-tab-id={props["data-tab-id"]}
      data-testid={props["data-testid"]}
    >
      {props.children}
    </a>
  ),
}));

const { contractMocks, sessionMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsTotals: vi.fn(), listProjects: vi.fn() },
  sessionMocks: { canEdit: true, workspaceName: "Acme Corp" as string | null },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../core/session/hooks.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/hooks.js")>();
  return {
    ...original,
    useWorkspace: (() => ({
      workspace: sessionMocks.workspaceName === null ? null : { name: sessionMocks.workspaceName },
      role: 20,
    })) as unknown as typeof original.useWorkspace,
    usePermissions: (() => ({
      role: 20,
      isMember: true,
      canEdit: sessionMocks.canEdit,
      isAdmin: false,
      isLoading: false,
    })) as unknown as typeof original.usePermissions,
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import { AnalyticsScreen } from "./AnalyticsScreen.js";

const WORKSPACE = "44444444-4444-4444-8444-444444444444";
const ALPHA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BETA = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

function projectRow(id: string, name: string, identifier: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: WORKSPACE,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

const PROJECTS = [projectRow(ALPHA, "Alpha", "ALP"), projectRow(BETA, "Beta", "BET")];

function renderScreen(
  props: { tabId?: string; projectIds?: readonly string[]; onScopeChange?: (ids: string[]) => void } = {}
): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AnalyticsScreen
        workspaceSlug="acme"
        tabId={props.tabId ?? "overview"}
        projectIds={props.projectIds ?? []}
        onScopeChange={props.onScopeChange ?? (() => {})}
      />
    </QueryClientProvider>
  );
}

function mockReads(options: { projects?: unknown; totals?: unknown; projectsFails?: boolean } = {}): void {
  contractMocks.listProjects.mockImplementation(() =>
    options.projectsFails
      ? Promise.reject(new Error("projects read failed"))
      : Promise.resolve(options.projects ?? PROJECTS)
  );
  contractMocks.getAnalyticsTotals.mockResolvedValue(options.totals ?? { total_users: { count: 3 } });
}

beforeEach(() => {
  vi.clearAllMocks();
  sessionMocks.canEdit = true;
  sessionMocks.workspaceName = "Acme Corp";
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("AnalyticsScreen", () => {
  it("renders the pending shell without tabs or the empty view", () => {
    contractMocks.listProjects.mockReturnValue(new Promise<never>(() => {}));
    renderScreen();
    expect(screen.getByTestId("analytics-pending")).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /No project/ })).not.toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Analytics");
    expect(document.title).toBe("Acme Corp - Analytics");
  });

  it("renders the empty view with an enabled action for members", async () => {
    mockReads({ projects: [] });
    renderScreen();
    await waitFor(() => expect(screen.getByRole("heading", { name: /No project/ })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Create a project" })).toBeEnabled();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(document.title).toBe("Acme Corp - Analytics");
  });

  it("treats archived and unjoined projects as no projects", async () => {
    mockReads({
      projects: [
        projectRow(ALPHA, "Alpha", "ALP", { archived_at: "2026-02-01T00:00:00.000Z" }),
        projectRow(BETA, "Beta", "BET", { member_role: null }),
      ],
    });
    renderScreen();
    await waitFor(() => expect(screen.getByRole("heading", { name: /No project/ })).toBeInTheDocument());
  });

  it("disables the creation action for guests", async () => {
    sessionMocks.canEdit = false;
    mockReads({ projects: [] });
    renderScreen();
    await waitFor(() => expect(screen.getByRole("heading", { name: /No project/ })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Create a project" })).toBeDisabled();
  });

  it("renders the overview toolbar, panel, and eleven cards", async () => {
    mockReads();
    renderScreen();
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Analytics");
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Work items" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByTestId("analytics-panel")).toHaveAttribute("id", "analytics-panel-overview");
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("All projects");
    await waitFor(() => expect(screen.getAllByTestId("analytics-headline-card")).toHaveLength(11));
    expect(contractMocks.getAnalyticsTotals).toHaveBeenCalledWith(expect.anything(), "acme", {
      tab: "overview",
      projectIds: [],
    });
  });

  it("renders the work-items panel with five cards", async () => {
    mockReads();
    renderScreen({ tabId: "work-items" });
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(screen.getByRole("tab", { name: "Work items" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("analytics-panel")).toHaveAttribute("id", "analytics-panel-work-items");
    await waitFor(() => expect(screen.getAllByTestId("analytics-headline-card")).toHaveLength(5));
    expect(document.title).toBe("Acme Corp - Analytics");
  });

  it("keeps the strip and picker with no panel for an unknown segment", async () => {
    mockReads();
    renderScreen({ tabId: "no-such-tab" });
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByRole("tab", { name: "Work items" })).toHaveAttribute("aria-selected", "false");
    expect(screen.queryByTestId("analytics-panel")).not.toBeInTheDocument();
    expect(screen.getByTestId("analytics-picker-trigger")).toBeInTheDocument();
    expect(contractMocks.getAnalyticsTotals).not.toHaveBeenCalled();
  });

  it("scopes the totals read to the resolved picker selection", async () => {
    mockReads();
    renderScreen({ projectIds: [BETA] });
    await waitFor(() => expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("Beta"));
    await waitFor(() =>
      expect(contractMocks.getAnalyticsTotals).toHaveBeenCalledWith(expect.anything(), "acme", {
        tab: "overview",
        projectIds: [BETA],
      })
    );
  });

  it("drops unresolvable ids from the label and the request", async () => {
    mockReads();
    renderScreen({ projectIds: ["ffffffff-ffff-4fff-8fff-ffffffffffff"] });
    await waitFor(() => expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("All projects"));
    await waitFor(() =>
      expect(contractMocks.getAnalyticsTotals).toHaveBeenCalledWith(expect.anything(), "acme", {
        tab: "overview",
        projectIds: [],
      })
    );
  });

  it("reports picker toggles through onScopeChange", async () => {
    const user = userEvent.setup();
    const onScopeChange = vi.fn();
    mockReads();
    renderScreen({ onScopeChange });
    await waitFor(() => expect(screen.getByTestId("analytics-picker-trigger")).toBeInTheDocument());
    await user.click(screen.getByTestId("analytics-picker-trigger"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.click(screen.getByRole("option", { name: "Beta" }));
    expect(onScopeChange).toHaveBeenCalledWith([BETA]);
  });

  it("retries the projects read from the error state", async () => {
    const user = userEvent.setup();
    contractMocks.listProjects.mockRejectedValueOnce(new Error("projects read failed")).mockResolvedValue(PROJECTS);
    contractMocks.getAnalyticsTotals.mockResolvedValue({});
    renderScreen();
    await waitFor(() => expect(screen.getByTestId("analytics-error")).toBeInTheDocument());
    expect(screen.getByText("Couldn't load projects.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(contractMocks.listProjects).toHaveBeenCalledTimes(2);
  });

  it("falls back to the slug in the title while the workspace name is missing", async () => {
    sessionMocks.workspaceName = null;
    mockReads();
    renderScreen();
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(document.title).toBe("acme - Analytics");
  });
});
