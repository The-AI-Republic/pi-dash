// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project shell (NEWFRONT-540, RUN-001/RUN-002/RUN-005). The project twin
// decides access from the workspace and project role records, renders the
// breadcrumb chrome (leaf "AI Workers", link ending in /runners/) and only
// starts the rail read once allowed.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import type { Project, Workspace } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode; "aria-current"?: "page" }) => (
      <a href={props.to} aria-current={props["aria-current"]}>
        {props.children}
      </a>
    ),
    useLocation: () => ({ pathname: "/acme/projects/p-1/runners" }),
  };
});

const { createAppClient, resetAppClient } = await import("../../core/api/client.js");
const { sessionKeys, setSessionClient } = await import("../../core/session/index.js");
const { authKeys } = await import("../auth/index.js");
const { runnersKeys } = await import("./api/keys.js");
const { PROJECT_RUNNERS_CRUMB, RunnersProjectShell } = await import("./RunnersProjectShell.js");
const { RUNNERS_DENIED_HEADING } = await import("./components/NotAuthorizedView.js");

function workspace(role: number): Workspace {
  return { id: "ws-id", name: "Acme", slug: "acme", logo_url: null, total_members: 2, role } as Workspace;
}

function renderShell(args: {
  workspaceRole: number;
  projectRoles: Record<string, number>;
  projectName?: string;
}): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  queryClient.setQueryData(sessionKeys.workspaces, [workspace(args.workspaceRole)]);
  queryClient.setQueryData(authKeys.projectRoles("acme"), args.projectRoles);
  queryClient.setQueryData(authKeys.project("acme", "p-1"), {
    id: "p-1",
    name: args.projectName ?? "Alpha",
  } as Project);
  queryClient.setQueryData(runnersKeys.list("acme", "p-1"), [
    { id: "r-1", name: "alpha", status: "online", enrolled_at: null },
  ]);
  render(
    <QueryClientProvider client={queryClient}>
      <RunnersProjectShell workspaceSlug="acme" workspaceId="ws-id" projectId="p-1">
        <p>project child</p>
      </RunnersProjectShell>
    </QueryClientProvider>
  );
}

beforeEach(() => {
  setSessionClient({} as never);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("RunnersProjectShell", () => {
  it("pins the breadcrumb leaf and link shape", () => {
    expect(PROJECT_RUNNERS_CRUMB).toBe("AI Workers");
  });

  it("admits a project member and renders the breadcrumb chrome", () => {
    renderShell({ workspaceRole: 5, projectRoles: { "p-1": 15 } });
    expect(screen.getByText("project child")).toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    const leaf = screen.getByRole("link", { name: /AI Workers/ });
    expect(leaf).toHaveAttribute("href", "/acme/projects/p-1/runners/");
    expect(screen.getByTestId("runners-rail")).toBeInTheDocument();
  });

  it("admits a workspace admin without a project entry", () => {
    renderShell({ workspaceRole: 20, projectRoles: {} });
    expect(screen.getByText("project child")).toBeInTheDocument();
  });

  it("denies a workspace guest inside the project chrome", () => {
    renderShell({ workspaceRole: 5, projectRoles: {} });
    expect(screen.getByRole("heading", { name: RUNNERS_DENIED_HEADING })).toBeInTheDocument();
    // The chrome keeps the breadcrumb; the rail and content do not render.
    expect(screen.getByRole("link", { name: /AI Workers/ })).toBeInTheDocument();
    expect(screen.queryByTestId("runners-rail")).toBeNull();
    expect(screen.queryByText("project child")).toBeNull();
  });

  it("denies a project guest", () => {
    renderShell({ workspaceRole: 5, projectRoles: { "p-1": 5 } });
    expect(screen.getByRole("heading", { name: RUNNERS_DENIED_HEADING })).toBeInTheDocument();
  });

  it("renders the chrome before the project row lands", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
    queryClient.setQueryData(sessionKeys.workspaces, [workspace(20)]);
    queryClient.setQueryData(authKeys.projectRoles("acme"), {});
    render(
      <QueryClientProvider client={queryClient}>
        <RunnersProjectShell workspaceSlug="acme" workspaceId="ws-id" projectId="p-1">
          <p>project child</p>
        </RunnersProjectShell>
      </QueryClientProvider>
    );
    // The project name is chrome, not a gate input: it renders empty, and
    // the area still mounts.
    expect(screen.getByRole("link", { name: /AI Workers/ })).toBeInTheDocument();
    expect(screen.getByText("project child")).toBeInTheDocument();
  });
});
