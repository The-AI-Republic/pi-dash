// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners routes, workspace scope (NEWFRONT-540, RUN-002/RUN-003/RUN-010).
// The layout resolves the workspace id before mounting the shell; the child
// routes mount the shared screen or a section placeholder so every tab and
// rail link deep-links.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { Workspace } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const params = { value: { ws: "acme" } as Record<string, string> };
const shellProps: Array<Record<string, unknown>> = [];
const screenProps: Array<Record<string, unknown>> = [];

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, Outlet: () => <div data-testid="outlet" /> };
});
vi.mock("../../../../features/runners/index.js", () => ({
  RunnersWorkspaceShell: (props: Record<string, unknown>) => {
    shellProps.push(props);
    return <div data-testid="workspace-shell">{props["children"] as React.ReactNode}</div>;
  },
}));
vi.mock("../../../../features/runners/components/RunnersOverviewScreen.js", () => ({
  RunnersOverviewScreen: (props: Record<string, unknown>) => {
    screenProps.push(props);
    return <div data-testid="overview-screen" />;
  },
}));

const { sessionKeys, setSessionClient } = await import("../../../../core/session/index.js");
const { Route: LayoutRoute } = await import("./route.js");
const { Route: IndexRoute } = await import("./index.js");
const { Route: RunsRoute } = await import("./runs.js");
const { Route: ApprovalsRoute } = await import("./approvals.js");
const { Route: DetailRoute } = await import("./detail/$runnerId.js");
const { Route: ChatRoute } = await import("./chat/$runnerId.js");

function mount(Component: React.ComponentType | undefined, seed = true): void {
  if (!Component) throw new Error("route has no component");
  if (seed) {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: "ws-id", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 15 } satisfies Workspace,
    ]);
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    return;
  }
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Component />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  shellProps.length = 0;
  screenProps.length = 0;
  params.value = { ws: "acme" };
  vi.spyOn(LayoutRoute, "useParams").mockReturnValue(params.value);
  vi.spyOn(IndexRoute, "useParams").mockReturnValue(params.value);
  setSessionClient({} as never);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("workspace runners layout", () => {
  it("waits for the workspace row", () => {
    mount(LayoutRoute.options.component, false);
    expect(screen.queryByTestId("workspace-shell")).toBeNull();
  });

  it("hands the shell the slug and resolved workspace id", () => {
    mount(LayoutRoute.options.component);
    expect(shellProps[0]).toMatchObject({ workspaceSlug: "acme", workspaceId: "ws-id" });
    expect(screen.getByTestId("outlet")).toBeInTheDocument();
  });
});

describe("workspace runners overview", () => {
  it("mounts the shared screen with the workspace name", () => {
    mount(IndexRoute.options.component);
    expect(screenProps[0]).toEqual({ workspaceSlug: "acme", workspaceName: "Acme" });
  });

  it("renders nothing before the workspace row", () => {
    mount(IndexRoute.options.component, false);
    expect(screen.queryByTestId("overview-screen")).toBeNull();
  });
});

describe("workspace runners section routes", () => {
  it.each([
    ["Runs", RunsRoute],
    ["Approvals", ApprovalsRoute],
    ["Runner details", DetailRoute],
    ["Runner chat", ChatRoute],
  ])("mounts the %s section", (heading, route) => {
    mount(route.options.component);
    expect(screen.getByRole("heading", { name: heading })).toBeInTheDocument();
  });
});
