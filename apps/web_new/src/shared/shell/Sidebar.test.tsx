// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { Sidebar, type SidebarProps } from "./Sidebar.js";

function sidebar(overrides: Partial<SidebarProps> = {}) {
  const props: SidebarProps = {
    activeSlug: "acme",
    userLabel: "Ada Lovelace",
    userEmail: "ada@example.com",
    userAvatarSrc: null,
    userRole: 20,
    onYourWork: () => undefined,
    onUserSettings: () => undefined,
    onSignOut: () => undefined,
    onNavigateHref: () => undefined,
    ...overrides,
  };
  return <Sidebar {...props} />;
}

/**
 * Mount the sidebar inside a minimal router whose workspace tree carries
 * the migrated screens the personal rows link to. The Drafts row is a
 * migrated route (F-09), so it renders as a router link rather than a
 * plain anchor; without a router that link cannot paint at all.
 */
function sidebarInRouter(overrides: Partial<SidebarProps> = {}) {
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "$ws",
    component: () => (
      <>
        {sidebar(overrides)}
        <Outlet />
      </>
    ),
  });
  const sectionRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "$section",
    component: () => null,
  });
  const nestedRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "$section/$rest",
    component: () => null,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([sectionRoute, nestedRoute])]),
    history: createMemoryHistory({ initialEntries: [overrides.currentPath ?? "/acme/"] }),
    defaultPreload: false,
  });
  return <RouterProvider router={router} />;
}

describe("Sidebar", () => {
  it("renders the brand and the account row", () => {
    render(sidebar());
    expect(screen.getByTestId("sidebar-brand")).toHaveTextContent("Pi Dash");
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
  });

  it("mounts no workspace switcher (NEWFRONT-1040)", () => {
    render(
      sidebar({
        editionEntries: [{ key: "apps", label: "Apps", href: "/acme/apps/" }],
        currentPath: "/acme/apps/",
      })
    );
    expect(screen.getByRole("link", { name: "Apps" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open workspace switcher" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("sidebar-workspace-switcher")).not.toBeInTheDocument();
  });

  it("renders the home row and marks it current on home", () => {
    render(sidebar({ currentPath: "/acme/" }));
    const home = screen.getByRole("link", { name: "Home" });
    expect(home).toHaveAttribute("href", "/acme");
    expect(home).toHaveAttribute("aria-current", "page");
  });

  it("navigates client-side from the home row", () => {
    const onNavigateHref = vi.fn();
    render(sidebar({ onNavigateHref, currentPath: "/acme/drafts/" }));
    const home = screen.getByRole("link", { name: "Home" });
    expect(home).not.toHaveAttribute("aria-current");
    fireEvent.click(home);
    expect(onNavigateHref).toHaveBeenCalledWith("/acme");
  });

  it("leaves modified home clicks to the browser", () => {
    const onNavigateHref = vi.fn();
    render(sidebar({ onNavigateHref }));
    fireEvent.click(screen.getByRole("link", { name: "Home" }), { metaKey: true, button: 0 });
    expect(onNavigateHref).not.toHaveBeenCalled();
  });

  it("renders the projects group slot", () => {
    render(sidebar({ projectsGroup: <p>group content</p> }));
    expect(screen.getByText("group content")).toBeInTheDocument();
  });

  it("renders the more and favorites slots after the projects group", () => {
    render(
      sidebar({
        projectsGroup: <p>group content</p>,
        moreGroup: <p>more content</p>,
        favoritesGroup: <p>favorites content</p>,
      })
    );
    const group = screen.getByText("group content");
    const more = screen.getByText("more content");
    const favorites = screen.getByText("favorites content");
    expect(group.compareDocumentPosition(more) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(more.compareDocumentPosition(favorites) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("signs out from the account menu", () => {
    const onSignOut = vi.fn();
    render(sidebar({ onSignOut }));
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it("omits deactivation from the user menu without a handler", async () => {
    const user = userEvent.setup();
    render(sidebar());
    await user.click(screen.getByTestId("sidebar-user-menu"));
    expect(await screen.findByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
    expect(await screen.findByRole("menuitem", { name: "Settings" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Deactivate account" })).not.toBeInTheDocument();
  });

  it("fires the deactivation handler from the user menu", async () => {
    const user = userEvent.setup();
    const onDeactivateAccount = vi.fn();
    render(sidebar({ onDeactivateAccount }));
    await user.click(screen.getByTestId("sidebar-user-menu"));
    await user.click(await screen.findByRole("menuitem", { name: "Deactivate account" }));
    expect(onDeactivateAccount).toHaveBeenCalledTimes(1);
  });

  it("renders the update control beside the account row when provided", () => {
    render(sidebar({ updateControl: <span>update ready</span> }));
    expect(screen.getByText("update ready")).toBeInTheDocument();
    const account = screen.getByTestId("sidebar-user-menu");
    const update = screen.getByText("update ready");
    expect(account).not.toContainElement(update);
  });

  it("mounts no plan badge unless the edition slot is filled (CLOUD-038)", () => {
    render(sidebar());
    expect(screen.queryByRole("button", { name: "Upgrade" })).not.toBeInTheDocument();
  });

  it("mounts the plan badge above the account row (CLOUD-038)", () => {
    render(sidebar({ planBadge: <button type="button">Upgrade</button> }));
    const badge = screen.getByRole("button", { name: "Upgrade" });
    expect(badge).toBeInTheDocument();
    const account = screen.getByTestId("sidebar-user-menu");
    expect(badge.compareDocumentPosition(account) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("routes the user-menu callbacks through", async () => {
    const user = userEvent.setup();
    const handlers = {
      onYourWork: vi.fn(),
      onUserSettings: vi.fn(),
    };
    render(sidebar(handlers));
    await user.click(screen.getByTestId("sidebar-user-menu"));
    await user.click(await screen.findByRole("menuitem", { name: "Your work" }));
    expect(handlers.onYourWork).toHaveBeenCalledTimes(1);
    await user.click(screen.getByTestId("sidebar-user-menu"));
    await user.click(await screen.findByRole("menuitem", { name: "Settings" }));
    expect(handlers.onUserSettings).toHaveBeenCalledTimes(1);
  });

  it("mounts the navigation landmark under the stable sidebar id", () => {
    const { container } = render(sidebar());
    const landmark = screen.getByRole("complementary", { name: "Workspace navigation" });
    // The parity drivers and the shell contract locate the sidebar by this id.
    expect(container.querySelector("#main-sidebar")).toBe(landmark);
  });

  it("lists personal entries above the projects group", async () => {
    render(
      sidebarInRouter({
        personalEntries: [{ key: "drafts", label: "Drafts", href: "/acme/drafts/" }],
        currentPath: "/acme/drafts/",
        projectsGroup: <p>group content</p>,
      })
    );
    await screen.findByRole("navigation", { name: "Personal" });
    const personal = screen.getByRole("navigation", { name: "Personal" });
    expect(personal).toHaveTextContent("Drafts");
    expect(personal.querySelector('[aria-current="page"]')).toHaveTextContent("Drafts");
    expect(screen.getByText("group content")).toBeInTheDocument();
  });

  it("renders quick actions under the header", () => {
    render(sidebar({ quickActions: <button type="button">New work item</button> }));
    expect(screen.getByTestId("sidebar-quick-actions")).toHaveTextContent("New work item");
  });

  it("leaves personal entries unmarked off their address or without a path", async () => {
    const personalEntries = [{ key: "drafts", label: "Drafts", href: "/acme/drafts/" }];
    const offAddress = render(sidebarInRouter({ personalEntries, currentPath: "/acme/" }));
    await screen.findByRole("link", { name: "Drafts" });
    expect(screen.getByRole("link", { name: "Drafts" })).not.toHaveAttribute("aria-current");
    offAddress.unmount();
    render(sidebarInRouter({ personalEntries }));
    await screen.findByRole("link", { name: "Drafts" });
    expect(screen.getByRole("link", { name: "Drafts" })).not.toHaveAttribute("aria-current");
  });

  it("renders no personal section without entries", () => {
    render(sidebar());
    expect(screen.queryByRole("navigation", { name: "Personal" })).not.toBeInTheDocument();
  });

  it("mounts no update control on web builds (SHELL-102)", () => {
    render(sidebar());
    expect(screen.queryByRole("button", { name: /update/i })).not.toBeInTheDocument();
  });

  it("renders nothing extra without edition entries (CLOUD-021 flag-off)", () => {
    const { container } = render(sidebar());
    expect(screen.getByRole("link", { name: "Home" })).toBeInTheDocument();
    expect(container.querySelector("#main-sidebar")?.textContent).not.toContain("Apps");
    expect(container.querySelectorAll('#main-sidebar a[href*="apps"]')).toHaveLength(0);
  });

  it("lists edition entries beside Home with their own icon (CLOUD-021)", () => {
    render(
      sidebar({
        editionEntries: [{ key: "apps", label: "Apps", href: "/acme/apps/" }],
        currentPath: "/acme/projects/p1/issues",
      })
    );
    const apps = screen.getByRole("link", { name: "Apps" });
    expect(apps).toHaveAttribute("href", "/acme/apps/");
    expect(apps.querySelector("svg")).not.toBeNull();
    expect(apps).not.toHaveAttribute("aria-current");
    const workspace = screen.getByRole("navigation", { name: "Workspace" });
    expect(workspace).toContainElement(apps);
  });

  it("marks the edition entry current on its route (CLOUD-021)", () => {
    render(
      sidebar({
        editionEntries: [{ key: "apps", label: "Apps", href: "/acme/apps/" }],
        currentPath: "/acme/apps/",
      })
    );
    expect(screen.getByRole("link", { name: "Apps" })).toHaveAttribute("aria-current", "page");
  });

  it("leaves edition entries unmarked without a path (CLOUD-021)", () => {
    render(sidebar({ editionEntries: [{ key: "apps", label: "Apps", href: "/acme/apps/" }] }));
    expect(screen.getByRole("link", { name: "Apps" })).not.toHaveAttribute("aria-current");
  });
});
