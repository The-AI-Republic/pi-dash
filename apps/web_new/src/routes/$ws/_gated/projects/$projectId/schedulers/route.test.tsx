// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers section shell (parity/specs/agents.md §5, AGT-021):
// List and Calendar tabs, exactly one marked current for a tab address and
// neither for an install detail underneath. The tab row carries the
// admin-only "New Scheduler" control on every tab (AGT-010).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { router, hooks } = vi.hoisted(() => ({
  router: { pathname: "/acme/projects/p1/schedulers/list" },
  hooks: {
    permissions: { isAdmin: false },
    roles: { isSuccess: true, isPending: false, data: {} as Record<string, number> },
    membership: { isSuccess: true, isPending: false },
    bindings: { data: [] as SchedulerBinding[] | undefined, refetch: () => Promise.resolve() },
  },
}));

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: ({ to, children, ...rest }: { to: string; children?: React.ReactNode }) => (
      <a href={to} {...rest}>
        {children}
      </a>
    ),
    Outlet: () => <div data-testid="tab-body" />,
    useLocation: () => ({ pathname: router.pathname }),
  };
});
vi.mock("../../../../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../../../core/session/index.js")>();
  return { ...original, usePermissions: () => hooks.permissions };
});
vi.mock("../../../../../../features/auth/queries.js", () => ({
  useProjectRoles: () => hooks.roles,
  useWorkspaceMembership: () => hooks.membership,
}));
vi.mock("../../../../../../features/schedulers/installData.js", () => ({
  useSchedulerBindings: () => hooks.bindings,
  invalidateInstalls: () => Promise.resolve(),
}));
vi.mock("../../../../../../features/schedulers/InstallDialog.js", () => ({
  InstallDialog: ({
    open,
    canInstall,
    canAuthor,
    installedSchedulerIds,
  }: {
    open: boolean;
    canInstall: boolean;
    canAuthor: boolean;
    installedSchedulerIds: readonly string[];
  }) => (
    <div
      data-testid="install-dialog"
      data-open={open}
      data-can-install={canInstall}
      data-can-author={canAuthor}
      data-installed={installedSchedulerIds.join(",")}
    />
  ),
}));

import { Route, schedulerSectionTabs } from "./route.js";

afterEach(() => {
  router.pathname = "/acme/projects/p1/schedulers/list";
  hooks.permissions = { isAdmin: false };
  hooks.roles = { isSuccess: true, isPending: false, data: {} };
  hooks.membership = { isSuccess: true, isPending: false };
  hooks.bindings = { data: [], refetch: () => Promise.resolve() };
});

const BASE = "/acme/projects/p1/schedulers";

describe("schedulerSectionTabs", () => {
  it("marks List current on the list address", () => {
    expect(schedulerSectionTabs(`${BASE}/list`, BASE)).toEqual([
      { key: "list", label: "List", href: `${BASE}/list`, active: true },
      { key: "calendar", label: "Calendar", href: `${BASE}/calendar`, active: false },
    ]);
  });

  it("marks Calendar current on the calendar address", () => {
    const tabs = schedulerSectionTabs(`${BASE}/calendar`, BASE);
    expect(tabs.map((tab) => tab.active)).toEqual([false, true]);
  });

  it("reads the bare section address as Calendar", () => {
    expect(schedulerSectionTabs(BASE, BASE).find((tab) => tab.active)?.key).toBe("calendar");
    expect(schedulerSectionTabs(`${BASE}/`, BASE).find((tab) => tab.active)?.key).toBe("calendar");
  });

  it("tolerates a trailing slash on the list address", () => {
    expect(schedulerSectionTabs(`${BASE}/list/`, BASE).find((tab) => tab.active)?.key).toBe("list");
  });

  it("marks neither tab on an install detail page", () => {
    expect(schedulerSectionTabs(`${BASE}/b1`, BASE).some((tab) => tab.active)).toBe(false);
  });
});

// The section body is route-split, so it is reached through the route like
// a visitor does rather than as a module export (NEWFRONT-871, size gate).
const ProjectSchedulersSectionComponent = Route.options.component!;

function renderSection(pathname: string): void {
  vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: "p1" });
  router.pathname = pathname;
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProjectSchedulersSectionComponent />
    </QueryClientProvider>
  );
}

describe("ProjectSchedulersSectionComponent", () => {
  it("renders both tabs with the current one marked and the tab body below", () => {
    renderSection("/acme/projects/p1/schedulers/list");
    const nav = screen.getByRole("navigation", { name: "Scheduler views" });
    const list = screen.getByRole("link", { name: "List" });
    const calendar = screen.getByRole("link", { name: "Calendar" });
    expect(nav).toContainElement(list);
    expect(list).toHaveAttribute("href", "/acme/projects/p1/schedulers/list");
    expect(list).toHaveAttribute("aria-current", "page");
    expect(calendar).toHaveAttribute("href", "/acme/projects/p1/schedulers/calendar");
    expect(calendar).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("tab-body")).toBeInTheDocument();
  });

  it("moves the current mark to Calendar on that address", () => {
    renderSection("/acme/projects/p1/schedulers/calendar");
    expect(screen.getByRole("link", { name: "Calendar" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "List" })).not.toHaveAttribute("aria-current");
  });
});

describe("New Scheduler in the tab row", () => {
  it.each([
    ["List", "/acme/projects/p1/schedulers/list"],
    ["Calendar", "/acme/projects/p1/schedulers/calendar"],
    ["the bare section address", "/acme/projects/p1/schedulers"],
    ["an install detail page", "/acme/projects/p1/schedulers/b1"],
  ])("shows a project admin the control on %s and opens the install dialog", async (_label, pathname) => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
    const user = userEvent.setup();
    renderSection(pathname);
    const button = screen.getByRole("button", { name: "New Scheduler" });
    expect(screen.getByTestId("tab-body")).not.toContainElement(button);
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-open", "false");
    await user.click(button);
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-open", "true");
  });

  it("hands the dialog the caller's flags and the installed definitions", () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
    hooks.permissions = { isAdmin: true };
    hooks.bindings = { data: [{ id: "b1", scheduler: "s9" } as SchedulerBinding], refetch: () => Promise.resolve() };
    renderSection("/acme/projects/p1/schedulers/calendar");
    const dialog = screen.getByTestId("install-dialog");
    expect(dialog).toHaveAttribute("data-can-install", "true");
    expect(dialog).toHaveAttribute("data-can-author", "true");
    expect(dialog).toHaveAttribute("data-installed", "s9");
  });

  it("treats an unsettled install list as nothing installed", () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
    hooks.bindings = { data: undefined, refetch: () => Promise.resolve() };
    renderSection("/acme/projects/p1/schedulers/list");
    expect(screen.getByTestId("install-dialog")).toHaveAttribute("data-installed", "");
  });

  it.each(["/acme/projects/p1/schedulers/list", "/acme/projects/p1/schedulers/calendar"])(
    "shows a plain member no control on %s",
    (pathname) => {
      hooks.roles = { isSuccess: true, isPending: false, data: { p1: 15 } };
      renderSection(pathname);
      expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
      expect(screen.queryByTestId("install-dialog")).not.toBeInTheDocument();
    }
  );

  it("shows a workspace admin without project admin standing no control", () => {
    hooks.permissions = { isAdmin: true };
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 15 } };
    renderSection("/acme/projects/p1/schedulers/calendar");
    expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
  });

  it("shows no control while the standing reads are missing or failed", () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
    hooks.membership = { isSuccess: false, isPending: false };
    renderSection("/acme/projects/p1/schedulers/list");
    expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
  });
});
