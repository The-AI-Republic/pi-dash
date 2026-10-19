// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings shell and gates (PSET-001/PSET-003/PSET-004). Auth reads and
// router state are mocked so each case pins one gate outcome: pending,
// denial kind, role, narrow drawer, page gate.
import { ApiError } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  row: {
    status: "success",
    data: undefined as unknown,
    error: undefined as unknown,
    isPending: false,
    isSuccess: true,
    isError: false,
  },
  roles: {
    status: "success",
    data: undefined as unknown,
    isPending: false,
    isSuccess: true,
    isError: false,
  },
  membership: { status: "success", data: undefined as unknown, isPending: false },
  pathname: "/acme/settings/projects/223e4567-e89b-12d3-a456-426614174001/",
  narrow: false,
  join: { isPending: false, mutate: vi.fn() },
}));

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: harness.pathname } }),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => (
    <a
      href={props.to}
      className={props["className"] as string | undefined}
      {...({
        "aria-current": props["aria-current"],
        "aria-label": props["aria-label"],
        "data-testid": props["data-testid"],
        "data-active": props["data-active"],
      } as React.AnchorHTMLAttributes<HTMLAnchorElement>)}
    >
      {props.children as React.ReactNode}
    </a>
  ),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => harness.narrow,
}));

vi.mock("../../auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useProjectRow: () => harness.row,
  useProjectRoles: () => harness.roles,
  useWorkspaceMembership: () => harness.membership,
  useJoinProject: () => harness.join,
}));

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { currentSectionLabel, ProjectSettingsShell, ProjectSettingsTabGate } from "./ProjectSettingsLayout.js";

const PID = "223e4567-e89b-12d3-a456-426614174001";
const ROOT = `/acme/settings/projects/${PID}`;
const PROJECT = { id: PID, name: "Website relaunch", identifier: "WEB", logo_props: {} };

function renderShell(node?: React.ReactNode): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
        {node ?? <p>tab body</p>}
      </ProjectSettingsShell>
    </QueryClientProvider>
  );
}

function seed({
  role = ROLE_ADMIN,
  workspaceRole = ROLE_ADMIN,
}: { role?: number | null; workspaceRole?: number } = {}): void {
  harness.row = {
    status: "success",
    data: PROJECT,
    error: undefined,
    isPending: false,
    isSuccess: true,
    isError: false,
  };
  harness.roles = { status: "success", data: { [PID]: role }, isPending: false, isSuccess: true, isError: false };
  harness.membership = { status: "success", data: { role: workspaceRole }, isPending: false };
}

beforeEach(() => {
  harness.pathname = `${ROOT}/`;
  harness.narrow = false;
  harness.join = { isPending: false, mutate: vi.fn() };
  seed();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("currentSectionLabel", () => {
  it("names the tab owning the address, its sub-address, or nothing", () => {
    expect(currentSectionLabel(`${ROOT}/`, "acme", PID)).toBe("General");
    expect(currentSectionLabel(`${ROOT}/members`, "acme", PID)).toBe("Members");
    expect(currentSectionLabel(`${ROOT}/features/cycles`, "acme", PID)).toBe("Cycles");
    expect(currentSectionLabel(`${ROOT}/features/cycles/abc`, "acme", PID)).toBe("Cycles");
    expect(currentSectionLabel(`${ROOT}/nope`, "acme", PID)).toBeUndefined();
    expect(currentSectionLabel("/elsewhere", "acme", PID)).toBe("General");
  });
});

describe("ProjectSettingsShell", () => {
  it("renders nothing while the project or roles resolve", () => {
    harness.row = {
      status: "pending",
      data: undefined,
      error: undefined,
      isPending: true,
      isSuccess: false,
      isError: false,
    };
    const { container } = render(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
          <p>tab body</p>
        </ProjectSettingsShell>
      </QueryClientProvider>
    );
    expect(container).toBeEmptyDOMElement();
    harness.row = {
      status: "success",
      data: PROJECT,
      error: undefined,
      isPending: false,
      isSuccess: true,
      isError: false,
    };

    harness.roles = { status: "pending", data: undefined, isPending: true, isSuccess: false, isError: false };
    harness.membership = { status: "success", data: { role: ROLE_ADMIN }, isPending: true };
    const second = render(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
          <p>tab body</p>
        </ProjectSettingsShell>
      </QueryClientProvider>
    );
    expect(second.container).toBeEmptyDOMElement();
    harness.roles = {
      status: "success",
      data: { [PID]: ROLE_ADMIN },
      isPending: false,
      isSuccess: true,
      isError: false,
    };
    harness.membership = { status: "success", data: { role: ROLE_ADMIN }, isPending: false };
  });

  it("renders the rail, the page body and the role caption", () => {
    renderShell();
    expect(screen.getByTestId("project-settings-shell")).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-sidebar")).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "Settings content" })).toHaveTextContent("tab body");
    expect(screen.getByText("Admin")).toBeInTheDocument();
  });

  it("shows a workspace admin the admin rail and caption over their project row", () => {
    seed({ role: ROLE_MEMBER, workspaceRole: ROLE_ADMIN });
    renderShell();
    expect(screen.getByText("Admin")).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-tab-schedulers")).toBeInTheDocument();
    expect(screen.getByTestId("project-settings-group-execution")).toBeInTheDocument();
  });

  it("hides the rail entries for a viewer without a role but still renders the body", () => {
    seed({ role: null, workspaceRole: ROLE_MEMBER });
    renderShell();
    expect(screen.queryByTestId("project-settings-sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("main", { name: "Settings content" })).toHaveTextContent("tab body");
  });

  it("keeps a failed role read from guessing a standing", () => {
    harness.roles = { status: "error", data: undefined, isPending: false, isSuccess: false, isError: true };
    renderShell();
    expect(screen.queryByTestId("project-settings-sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("main", { name: "Settings content" })).toBeInTheDocument();
  });

  it("asks a non-member to join and calls the join mutation", () => {
    harness.row = {
      status: "error",
      data: undefined,
      error: new ApiError({ status: 409, code: "conflict", message: "join" }),
      isPending: false,
      isSuccess: false,
      isError: true,
    };
    renderShell();
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "join");
    fireEvent.click(screen.getByRole("button", { name: "Join project" }));
    expect(harness.join.mutate).toHaveBeenCalledWith({ slug: "acme", projectId: PID });
  });

  it("offers no access to a refused non-admin and not-found to unknown ids", () => {
    seed({ workspaceRole: ROLE_MEMBER });
    harness.row = {
      status: "error",
      data: undefined,
      error: new ApiError({ status: 403, code: "forbidden", message: "no" }),
      isPending: false,
      isSuccess: false,
      isError: true,
    };
    const refused = render(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
          <p>tab body</p>
        </ProjectSettingsShell>
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "no-access");
    refused.unmount();

    harness.row = {
      status: "error",
      data: undefined,
      error: new ApiError({ status: 404, code: "not_found", message: "missing" }),
      isPending: false,
      isSuccess: false,
      isError: true,
    };
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
          <p>tab body</p>
        </ProjectSettingsShell>
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "not-found");
    expect(screen.getByRole("heading", { name: "Project not found" })).toBeInTheDocument();
  });

  it("joins a workspace admin refused a private project", () => {
    harness.row = {
      status: "error",
      data: undefined,
      error: new ApiError({ status: 403, code: "forbidden", message: "no" }),
      isPending: false,
      isSuccess: false,
      isError: true,
    };
    renderShell();
    expect(screen.getByTestId("project-settings-restriction")).toHaveAttribute("data-restriction", "join");
  });

  it("shows the join mutation pending state", () => {
    harness.row = {
      status: "error",
      data: undefined,
      error: new ApiError({ status: 409, code: "conflict", message: "join" }),
      isPending: false,
      isSuccess: false,
      isError: true,
    };
    harness.join = { isPending: true, mutate: vi.fn() };
    renderShell();
    expect(screen.getByRole("button", { name: "Joining…" })).toBeDisabled();
  });

  it("draws the rail as a narrow drawer and closes it on navigation or backdrop", () => {
    harness.narrow = true;
    renderShell();
    expect(screen.queryByTestId("project-settings-drawer")).not.toBeInTheDocument();
    const trigger = screen.getByRole("button", { name: "Open settings navigation" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("project-settings-tab-general")).toBeInTheDocument();
    fireEvent.click(trigger);
    expect(screen.getByTestId("project-settings-drawer")).toBeInTheDocument();
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(screen.getByRole("button", { name: "Close settings navigation" }));
    expect(screen.queryByTestId("project-settings-drawer")).not.toBeInTheDocument();

    cleanup();
    harness.pathname = `${ROOT}/nope`;
    renderShell();
    expect(screen.getAllByText("Website relaunch").length).toBeGreaterThan(0);
  });
});

describe("ProjectSettingsTabGate", () => {
  function renderGate(tabKey: "members" | "github" = "members"): void {
    render(
      <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
        <ProjectSettingsTabGate tabKey={tabKey}>
          <p>tab body</p>
        </ProjectSettingsTabGate>
      </ProjectSettingsShell>
    );
  }

  it("keeps the rail beside a refusal", () => {
    seed({ role: ROLE_GUEST, workspaceRole: ROLE_GUEST });
    renderGate();
    expect(screen.getByTestId("project-settings-denied")).toHaveTextContent("Not authorized");
    expect(screen.getByTestId("project-settings-sidebar")).toBeInTheDocument();
    expect(screen.queryByText("tab body")).not.toBeInTheDocument();
  });

  it("admits the members page to a project guest who administers the workspace", () => {
    seed({ role: ROLE_GUEST, workspaceRole: ROLE_ADMIN });
    renderGate();
    expect(screen.queryByTestId("project-settings-denied")).not.toBeInTheDocument();
    expect(screen.getByText("tab body")).toBeInTheDocument();
  });

  it("denies the wider repository page to guests and admits members", () => {
    seed({ role: ROLE_GUEST, workspaceRole: ROLE_GUEST });
    const denied = render(
      <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
        <ProjectSettingsTabGate tabKey="github">
          <p>repo body</p>
        </ProjectSettingsTabGate>
      </ProjectSettingsShell>
    );
    expect(screen.getByTestId("project-settings-denied")).toBeInTheDocument();
    denied.unmount();

    seed({ role: ROLE_MEMBER, workspaceRole: ROLE_MEMBER });
    render(
      <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
        <ProjectSettingsTabGate tabKey="github">
          <p>repo body</p>
        </ProjectSettingsTabGate>
      </ProjectSettingsShell>
    );
    expect(screen.getByText("repo body")).toBeInTheDocument();
  });

  it("renders unknown keys through", () => {
    seed({ role: ROLE_GUEST });
    render(
      <ProjectSettingsShell workspaceSlug="acme" projectId={PID}>
        <ProjectSettingsTabGate tabKey={"nope" as never}>
          <p>body</p>
        </ProjectSettingsTabGate>
      </ProjectSettingsShell>
    );
    expect(screen.getByText("body")).toBeInTheDocument();
  });
});
