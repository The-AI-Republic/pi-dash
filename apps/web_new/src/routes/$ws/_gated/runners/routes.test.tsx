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
const chatProps: Array<Record<string, unknown>> = [];
const chatNavigate = vi.fn();

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, Outlet: () => <div data-testid="outlet" /> };
});
vi.mock("../../../../features/runners/area.js", () => ({
  useRunnersArea: () => ({
    scope: { workspaceSlug: "acme" },
    workspaceId: "ws-id",
    access: "allowed",
    allowed: true,
    base: "/acme/runners",
    path: (child?: string) => (child === undefined ? "/acme/runners" : `/acme/runners/${child}`),
  }),
}));
vi.mock("../../../../features/runners/chat/RunnerChatPage.js", () => ({
  RunnerChatPage: (props: Record<string, unknown>) => {
    chatProps.push(props);
    return <div data-testid="runner-chat-page" />;
  },
}));
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
const runsProps: Array<Record<string, unknown>> = [];
vi.mock("../../../../features/runners/runs/RunsScreen.js", () => ({
  RunsScreen: (props: Record<string, unknown>) => {
    runsProps.push(props);
    return <h1>Runs</h1>;
  },
}));

const { sessionKeys, setSessionClient } = await import("../../../../core/session/index.js");
const { Route: LayoutRoute } = await import("./route.js");
const { Route: IndexRoute } = await import("./index.js");
const { Route: RunsRoute } = await import("./runs.index.js");
const { Route: RunDetailRoute } = await import("./runs.$id.js");
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
  chatProps.length = 0;
  chatNavigate.mockReset();
  runsProps.length = 0;
  vi.spyOn(LayoutRoute, "useParams").mockReturnValue(params.value);
  vi.spyOn(IndexRoute, "useParams").mockReturnValue(params.value);
  vi.spyOn(RunsRoute, "useSearch").mockReturnValue({ page: 2 });
  vi.spyOn(RunsRoute, "useParams").mockReturnValue({ ws: "acme" });
  vi.spyOn(RunDetailRoute, "useSearch").mockReturnValue({ page: 2 });
  vi.spyOn(RunDetailRoute, "useParams").mockReturnValue({ ws: "acme", id: "run-1" });
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
  ])("mounts the %s section", (heading, route) => {
    mount(route.options.component);
    expect(screen.getByRole("heading", { name: heading })).toBeInTheDocument();
  });

  it("passes the URL page and no selection to the runs screen", () => {
    mount(RunsRoute.options.component);
    expect(runsProps[0]).toEqual({ search: { page: 2 }, selectedRunId: null });
  });

  it("normalizes the runs page from the address", () => {
    vi.spyOn(RunsRoute, "useSearch").mockReturnValue({ page: "3" });
    mount(RunsRoute.options.component);
    expect(runsProps.at(-1)).toEqual({ search: { page: "3" }, selectedRunId: null });
    vi.spyOn(RunsRoute, "useSearch").mockReturnValue({ page: "nope" });
    mount(RunsRoute.options.component);
    expect(runsProps.at(-1)).toEqual({ search: { page: "nope" }, selectedRunId: null });
  });

  it("passes the run id from the address to the same screen", () => {
    mount(RunDetailRoute.options.component);
    expect(runsProps.at(-1)).toEqual({ search: { page: 2 }, selectedRunId: "run-1" });
  });

  it("mounts the chat page with the runner from the address", () => {
    vi.spyOn(ChatRoute, "useParams").mockReturnValue({ ws: "acme", runnerId: "r-1" });
    vi.spyOn(ChatRoute, "useSearch").mockReturnValue({});
    vi.spyOn(ChatRoute, "useNavigate").mockReturnValue(chatNavigate as never);
    mount(ChatRoute.options.component);
    expect(chatProps[0]).toMatchObject({ workspaceSlug: "acme", runnerId: "r-1", requestedSessionId: null });
  });

  it("passes an explicit session deep link through", () => {
    vi.spyOn(ChatRoute, "useParams").mockReturnValue({ ws: "acme", runnerId: "r-1" });
    vi.spyOn(ChatRoute, "useSearch").mockReturnValue({ sessionId: "s-9" });
    vi.spyOn(ChatRoute, "useNavigate").mockReturnValue(chatNavigate as never);
    mount(ChatRoute.options.component);
    expect(chatProps[0]).toMatchObject({ requestedSessionId: "s-9" });
  });

  it("treats a missing session parameter as no deep link", () => {
    vi.spyOn(ChatRoute, "useParams").mockReturnValue({ ws: "acme", runnerId: "r-1" });
    vi.spyOn(ChatRoute, "useSearch").mockReturnValue({ sessionId: undefined });
    vi.spyOn(ChatRoute, "useNavigate").mockReturnValue(chatNavigate as never);
    mount(ChatRoute.options.component);
    expect(chatProps[0]).toMatchObject({ requestedSessionId: null });
  });

  it("rewrites the address when an entry is selected", () => {
    vi.spyOn(ChatRoute, "useParams").mockReturnValue({ ws: "acme", runnerId: "r-1" });
    vi.spyOn(ChatRoute, "useSearch").mockReturnValue({});
    vi.spyOn(ChatRoute, "useNavigate").mockReturnValue(chatNavigate as never);
    mount(ChatRoute.options.component);
    const selectSession = chatProps[0]?.["onSelectSession"] as ((id: string) => void) | undefined;
    selectSession?.("s-2");
    expect(chatNavigate).toHaveBeenCalledWith({
      to: "/$ws/runners/chat/$runnerId",
      params: { ws: "acme", runnerId: "r-1" },
      search: { sessionId: "s-2" },
      replace: true,
    });
  });

  it("validates the session search parameter", () => {
    const validate = ChatRoute.options.validateSearch as (input: Record<string, unknown>) => unknown;
    expect(validate({ sessionId: "s-1" })).toEqual({ sessionId: "s-1" });
    expect(validate({ sessionId: "" })).toEqual({});
    expect(validate({ sessionId: 9 })).toEqual({});
    expect(validate({})).toEqual({});
  });
});
