// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners routes, project scope (NEWFRONT-540, RUN-001/RUN-002/RUN-003).
// The project twin mounts the same shell and screen with the project id in
// the scope, so every link and read lands in the project tree.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { Workspace } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const shellProps: Array<Record<string, unknown>> = [];
const screenProps: Array<Record<string, unknown>> = [];

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Outlet: () => <div data-testid="outlet" />,
    useLocation: () => ({ pathname: "/acme/projects/p-1/runners" }),
  };
});
vi.mock("../../../../../../features/runners/index.js", () => ({
  RunnersProjectShell: (props: Record<string, unknown>) => {
    shellProps.push(props);
    return <div data-testid="project-shell">{props["children"] as React.ReactNode}</div>;
  },
}));
// The screen file also imports the area context it renders under; the area
// module is mocked below so the screen container can mount bare.

vi.mock("../../../../../../features/runners/area.js", () => ({
  useRunnersArea: () => ({
    scope: { workspaceSlug: "acme", projectId: "p-1" },
    workspaceId: "ws-id",
    access: "allowed",
    allowed: true,
    base: "/acme/projects/p-1/runners",
    path: (child?: string) =>
      child === undefined ? "/acme/projects/p-1/runners" : `/acme/projects/p-1/runners/${child}`,
  }),
}));

const { sessionKeys, setSessionClient } = await import("../../../../../../core/session/index.js");
const { createAppClient, resetAppClient } = await import("../../../../../../core/api/client.js");
vi.mock("../../../../../../features/runners/components/RunnersOverviewScreen.js", () => ({
  RunnersOverviewScreen: (props: Record<string, unknown>) => {
    screenProps.push(props);
    return <div data-testid="overview-screen" />;
  },
}));

const { Route: LayoutRoute } = await import("./route.js");
const { Route: IndexRoute } = await import("./index.js");
const { Route: RunsRoute } = await import("./runs.js");
const { Route: ApprovalsRoute } = await import("./approvals.js");
const { Route: DetailRoute } = await import("./detail/$runnerId.js");
const { Route: ChatRoute } = await import("./chat/$runnerId.js");

const params = { ws: "acme", projectId: "p-1" };

function mount(Component: React.ComponentType | undefined, seed = true): void {
  if (!Component) throw new Error("route has no component");
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  if (seed) {
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: "ws-id", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 15 } satisfies Workspace,
    ]);
  }
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  shellProps.length = 0;
  screenProps.length = 0;
  for (const route of [LayoutRoute, IndexRoute]) vi.spyOn(route, "useParams").mockReturnValue(params);
  setSessionClient({} as never);
  // The shell's reads resolve from the seeded cache; the factory only has
  // to hand the hooks a client.
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("project runners layout", () => {
  it("waits for the workspace row", () => {
    mount(LayoutRoute.options.component, false);
    expect(screen.queryByTestId("project-shell")).toBeNull();
  });

  it("hands the project shell the slug, workspace id and project id", () => {
    mount(LayoutRoute.options.component);
    expect(shellProps[0]).toMatchObject({ workspaceSlug: "acme", workspaceId: "ws-id", projectId: "p-1" });
    expect(screen.getByTestId("outlet")).toBeInTheDocument();
  });
});

describe("project runners overview", () => {
  it("mounts the shared screen with the workspace name", () => {
    mount(IndexRoute.options.component);
    expect(screenProps[0]).toEqual({ workspaceSlug: "acme", workspaceName: "Acme" });
  });

  it("mounts without a workspace name while the row is missing", () => {
    mount(IndexRoute.options.component, false);
    expect(screenProps[0]).toEqual({ workspaceSlug: "acme", workspaceName: undefined });
  });
});

describe("project runners section routes", () => {
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
