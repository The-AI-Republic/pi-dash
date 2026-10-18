// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace shell (NEWFRONT-540, RUN-001/RUN-004). Reads the caller's role,
// starts the rail read only once access is allowed, and passes the router's
// pathname to the tab strip.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import type { Workspace } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const pathname = { value: "/acme/runners" };

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode; "aria-current"?: "page" }) => (
      <a href={props.to} aria-current={props["aria-current"]}>
        {props.children}
      </a>
    ),
    useLocation: () => ({ pathname: pathname.value }),
  };
});

const { createAppClient, resetAppClient } = await import("../../core/api/client.js");
const { sessionKeys, setSessionClient } = await import("../../core/session/index.js");
const { runnersKeys } = await import("./api/keys.js");
const { RunnersWorkspaceShell } = await import("./RunnersWorkspaceShell.js");
const { RUNNERS_DENIED_HEADING } = await import("./components/NotAuthorizedView.js");

function workspace(role: number): Workspace {
  return { id: "ws-id", name: "Acme", slug: "acme", logo_url: null, total_members: 2, role } as Workspace;
}

function renderShell(role: number | null, seedList = true): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  if (role !== null) queryClient.setQueryData(sessionKeys.workspaces, [workspace(role)]);
  if (seedList) {
    queryClient.setQueryData(runnersKeys.list("acme", null), [
      { id: "r-1", name: "alpha", status: "online", enrolled_at: null },
    ]);
  }
  render(
    <QueryClientProvider client={queryClient}>
      <RunnersWorkspaceShell workspaceSlug="acme" workspaceId="ws-id">
        <p>overview child</p>
      </RunnersWorkspaceShell>
    </QueryClientProvider>
  );
}

beforeEach(() => {
  pathname.value = "/acme/runners";
  // Every read resolves from the seeded cache; the client only exists so
  // the hooks can ask for one.
  setSessionClient({} as never);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("RunnersWorkspaceShell", () => {
  it("admits a member and shows the rail from the cached list", () => {
    renderShell(15);
    expect(screen.getByText("AI Agents")).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "Runners content" })).toContainElement(screen.getByText("overview child"));
    // Both the rail and the tab strip carry an Overview link; the tab is
    // the one inside the tab strip.
    const tabs = within(screen.getByTestId("runners-tabs"));
    expect(tabs.getByRole("link", { name: "Overview" })).toHaveAttribute("aria-current", "page");
  });

  it("renders an empty rail while the runner list is still absent", () => {
    renderShell(15, false);
    expect(screen.getByText("AI Agents")).toBeInTheDocument();
    expect(screen.getByText("No runners connected yet.")).toBeInTheDocument();
  });

  it("denies a guest inside the area", () => {
    renderShell(5);
    expect(screen.getByRole("heading", { name: RUNNERS_DENIED_HEADING })).toBeInTheDocument();
    expect(screen.queryByText("overview child")).toBeNull();
  });

  it("keeps the tab marker on the URL for child routes", () => {
    pathname.value = "/acme/runners/runs";
    renderShell(20);
    const tabs = within(screen.getByTestId("runners-tabs"));
    expect(tabs.getByRole("link", { name: "Runs" })).toHaveAttribute("aria-current", "page");
    expect(tabs.getByRole("link", { name: "Overview" })).not.toHaveAttribute("aria-current");
  });
});
