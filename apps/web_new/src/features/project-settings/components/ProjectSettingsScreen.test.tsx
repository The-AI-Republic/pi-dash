// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Screen composition (PSET-001/PSET-004): the shell wraps the body, the
// page header uses the registry title, and screens that bring their own
// header skip it.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  role: 20,
  workspaceRole: 20,
  pathname: "/acme/settings/projects/223e4567-e89b-12d3-a456-426614174001/",
}));

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: harness.pathname } }),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => (
    <a
      href={props.to}
      aria-label={props["aria-label"] as string | undefined}
      data-testid={props["data-testid"] as string | undefined}
      className={props["className"] as string | undefined}
      data-active={props["data-active"] as string | undefined}
    >
      {props.children as React.ReactNode}
    </a>
  ),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => false,
}));

vi.mock("../../auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectRow: () => ({
    status: "success",
    data: { id: "223e4567-e89b-12d3-a456-426614174001", name: "Website relaunch", identifier: "WEB", logo_props: {} },
    isPending: false,
    isSuccess: true,
  }),
  useProjectRoles: () => ({
    status: "success",
    data: { "223e4567-e89b-12d3-a456-426614174001": harness.role },
    isPending: false,
    isSuccess: true,
  }),
  useWorkspaceMembership: () => ({ status: "success", data: { role: harness.workspaceRole }, isPending: false }),
  useJoinProject: () => ({ isPending: false, mutate: vi.fn() }),
}));

import { ProjectSettingsScreen } from "./ProjectSettingsScreen.js";

const PID = "223e4567-e89b-12d3-a456-426614174001";

function renderScreen(props: { pageHeader?: boolean; tabKey?: "members" | "github" | "estimates" }): void {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProjectSettingsScreen
        workspaceSlug="acme"
        projectId={PID}
        tabKey={props.tabKey ?? "members"}
        {...(props.pageHeader === undefined ? {} : { pageHeader: props.pageHeader })}
      >
        <p>tab body</p>
      </ProjectSettingsScreen>
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.role = 20;
  harness.workspaceRole = 20;
});

describe("ProjectSettingsScreen", () => {
  it("renders the page header from the registry title", () => {
    renderScreen({});
    expect(screen.getByRole("heading", { level: 1, name: "Members" })).toBeInTheDocument();
    expect(screen.getByText("tab body")).toBeInTheDocument();
  });

  it("skips the header for screens that bring their own", () => {
    renderScreen({ pageHeader: false });
    expect(screen.queryByRole("heading", { level: 1 })).not.toBeInTheDocument();
    expect(screen.getByText("tab body")).toBeInTheDocument();
  });

  it("gates the body for the role", () => {
    harness.role = 15;
    renderScreen({ tabKey: "estimates" });
    expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument();
  });

  it("falls back to the key for an unknown tab", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectSettingsScreen workspaceSlug="acme" projectId={PID} tabKey={"nope" as never}>
          <p>body</p>
        </ProjectSettingsScreen>
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { level: 1, name: "nope" })).toBeInTheDocument();
  });
});
