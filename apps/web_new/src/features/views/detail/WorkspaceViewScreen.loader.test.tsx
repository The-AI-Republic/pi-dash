// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The workspace detail's issues mutation loader (NEWFRONT-933, ISS-005).
// The screen is an issues surface: its static defaults run display
// preference writes before any issues layout root mounts, so the provider
// has to sit at the screen's own edge. The regression was the render
// crash ("useIssuesLoader must be used under IssuesLoaderProvider") on
// the all-issues default. These cases keep the real preference-write hook
// mounted: the screen must render, and a write from its header tree must
// reach the server instead of throwing.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
// Only navigation and `Link` are replaced; the rest of the router stays
// real. The crumb link needs a live router, which this case does not mount.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => routerHarness.navigate,
  Link: () => <span data-testid="router-link" />,
}));

vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  usePermissions: () => ({ role: 15, isMember: true, canEdit: true, isAdmin: false, isLoading: false }),
  useMe: () => ({
    data: { id: "123e4567-e89b-12d3-a456-426614174000" },
    isPending: false,
    isError: false,
    refetch: () => Promise.resolve(),
  }),
  useWorkspace: () => ({ workspace: { name: "Acme", slug: "acme", role: 15 } }),
}));

// Both filter hosts ride their own chunks; this case is about the loader
// edge, so each stands in as a slot with nothing to render.
vi.mock("../../issues/filters/rich/lazy.js", () => ({
  LazyIssueFilters: ({
    children,
  }: {
    children: (parts: { control: React.ReactNode; row: React.ReactNode }) => React.ReactNode;
  }) => <>{children({ control: null, row: null })}</>,
}));
vi.mock("./WorkspaceDefaultFilters.js", () => ({
  WorkspaceDefaultFilters: ({
    children,
  }: {
    children: (parts: { control: React.ReactNode; row: React.ReactNode }) => React.ReactNode;
  }) => <>{children({ control: null, row: null })}</>,
}));

// The body list is the issues suite's subject; this screen only has to
// hand it the scoped query and mount without crashing.
const listHarness = vi.hoisted(() => ({ mounts: 0, label: "" }));
vi.mock("../../issues/global/GlobalIssuesScreen.js", () => ({
  GlobalIssuesScreen: (props: { label?: string }) => {
    listHarness.mounts += 1;
    listHarness.label = props.label ?? "";
    return <div data-testid="global-list" data-label={props.label ?? ""} />;
  },
}));

// The real Display control renders beside a probe that drives the same
// real preference write, so the write path is exercised through the
// screen's own tree rather than through a stand-in hook.
const displayHarness = vi.hoisted(() => ({
  props: null as null | { workspaceSlug: string; entity: unknown },
}));
vi.mock("../../issues/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../issues/index.js")>();
  type DisplayProps = Parameters<typeof original.DisplayControl>[0];
  type UpdateEntity = Parameters<typeof original.useUpdateDisplayPrefs>[1];
  function ProbeDisplay(props: DisplayProps): React.ReactElement {
    displayHarness.props = props;
    const { updateDisplayPrefs } = original.useUpdateDisplayPrefs(props.workspaceSlug, props.entity as UpdateEntity);
    return (
      <>
        <original.DisplayControl {...props} />
        <button type="button" data-testid="probe-write" onClick={() => updateDisplayPrefs({ showEmptyGroups: true })}>
          write
        </button>
      </>
    );
  }
  return { ...original, DisplayControl: ProbeDisplay };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { jsonResponse, makeView } from "../testing.js";
import { WorkspaceViewScreen, type WorkspacePeekAddress } from "./WorkspaceViewScreen.js";

const PREFS_PATH = "/api/workspaces/acme/user-properties/";

function userProperties(displayFilters: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    user: "123e4567-e89b-12d3-a456-426614174000",
    filters: {},
    display_filters: { layout: "spreadsheet", group_by: null, order_by: "-created_at", ...displayFilters },
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: 10,
    navigation_control_preference: "all",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
  };
}

let patches: Array<Record<string, unknown>>;

beforeEach(() => {
  routerHarness.navigate.mockReset();
  listHarness.mounts = 0;
  listHarness.label = "";
  displayHarness.props = null;
  patches = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const path = url.split("?")[0] ?? url;
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (path.endsWith(PREFS_PATH)) {
      if (init.method !== "PATCH") return jsonResponse(url, 200, userProperties());
      const body = JSON.parse(init.body ?? "{}") as {
        display_filters?: Record<string, unknown>;
        display_properties?: Record<string, unknown>;
      };
      patches.push(body);
      return jsonResponse(url, 200, {
        ...userProperties(),
        display_filters: body.display_filters ?? {},
        display_properties: body.display_properties ?? {},
      });
    }
    if (path.endsWith("/api/workspaces/acme/views/")) {
      return jsonResponse(url, 200, [makeView({ id: "323e4567-e89b-12d3-a456-426614174002", project: null })]);
    }
    if (path.endsWith("/api/workspaces/acme/projects/")) return jsonResponse(url, 200, []);
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

function peekAddress(): WorkspacePeekAddress {
  return { peekKey: null, peekProjectId: null, onPeekChange: () => undefined };
}

function renderAllIssues(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <WorkspaceViewScreen params={{ ws: "acme", viewId: "all-issues" }} peek={peekAddress()} />
    </QueryClientProvider>
  );
}

describe("WorkspaceViewScreen loader edge (NEWFRONT-933)", () => {
  it("renders the all-issues default instead of throwing for a missing provider", async () => {
    renderAllIssues();
    // The regression: the render threw the provider error before the
    // header, the display control or the body could commit.
    expect(await screen.findByTestId("workspace-view-header")).toBeInTheDocument();
    expect(await screen.findByTestId("global-list")).toHaveAttribute("data-label", "All work items");
    expect(screen.getByTestId("display-options-trigger")).toBeInTheDocument();
  });

  it("writes a display change from the header tree through the real loader", async () => {
    renderAllIssues();
    await waitFor(() => expect(listHarness.mounts).toBeGreaterThan(0));
    expect(displayHarness.props?.workspaceSlug).toBe("acme");
    fireEvent.click(await screen.findByTestId("probe-write"));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]?.["display_filters"]).toMatchObject({ show_empty_groups: true });
  });
});
