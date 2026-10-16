// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { Sidebar, type SidebarProps } from "./Sidebar.js";

const WORKSPACES = [
  { id: "ws-1", slug: "acme", name: "Acme" },
  { id: "ws-2", slug: "globex", name: "Globex" },
];

function sidebar(overrides: Partial<SidebarProps> = {}) {
  const props: SidebarProps = {
    workspaces: WORKSPACES,
    activeSlug: "acme",
    onSelectWorkspace: () => undefined,
    userLabel: "Ada Lovelace",
    onSignOut: () => undefined,
    onNavigateHref: () => undefined,
    ...overrides,
  };
  return <Sidebar {...props} />;
}

describe("Sidebar", () => {
  it("renders the brand and the account row", () => {
    render(sidebar());
    expect(screen.getByTestId("sidebar-brand")).toHaveTextContent("Pi Dash");
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
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

  it("switches workspaces from the switcher", () => {
    const onSelectWorkspace = vi.fn();
    render(sidebar({ onSelectWorkspace }));
    fireEvent.click(screen.getByRole("button", { name: "Switch workspace, current: Acme" }));
    expect(screen.getByRole("menuitem", { name: "Acme" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Globex" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Globex" }));
    expect(onSelectWorkspace).toHaveBeenCalledWith("globex");
  });

  it("falls back to the slug when the workspace row is missing", () => {
    render(sidebar({ activeSlug: "unknown" }));
    expect(screen.getByRole("button", { name: "Switch workspace, current: unknown" })).toBeInTheDocument();
  });

  it("renders the projects group slot", () => {
    render(sidebar({ projectsGroup: <p>group content</p> }));
    expect(screen.getByText("group content")).toBeInTheDocument();
  });

  it("signs out from the account menu", () => {
    const onSignOut = vi.fn();
    render(sidebar({ onSignOut }));
    fireEvent.click(screen.getByTestId("sidebar-account-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });

  it("offers account deactivation only when wired", () => {
    const first = render(sidebar());
    fireEvent.click(first.getByTestId("sidebar-account-menu"));
    expect(screen.queryByRole("menuitem", { name: "Deactivate account" })).not.toBeInTheDocument();
    first.unmount();

    const onDeactivateAccount = vi.fn();
    render(sidebar({ onDeactivateAccount }));
    fireEvent.click(screen.getByTestId("sidebar-account-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Deactivate account" }));
    expect(onDeactivateAccount).toHaveBeenCalledTimes(1);
  });

  it("renders the update control beside the account row when provided", () => {
    render(sidebar({ updateControl: <span>update ready</span> }));
    expect(screen.getByText("update ready")).toBeInTheDocument();
  });

  it("lists personal entries above the projects group", () => {
    render(
      sidebar({
        personalEntries: [{ key: "drafts", label: "Drafts", href: "/acme/drafts/" }],
        currentPath: "/acme/drafts/",
        projectsGroup: <p>group content</p>,
      })
    );
    const personal = screen.getByRole("navigation", { name: "Personal" });
    expect(personal).toHaveTextContent("Drafts");
    expect(personal.querySelector('[aria-current="page"]')).toHaveTextContent("Drafts");
    expect(screen.getByText("group content")).toBeInTheDocument();
  });

  it("renders quick actions under the header", () => {
    render(sidebar({ quickActions: <button type="button">New work item</button> }));
    expect(screen.getByTestId("sidebar-quick-actions")).toHaveTextContent("New work item");
  });

  it("leaves personal entries unmarked off their address or without a path", () => {
    const personalEntries = [{ key: "drafts", label: "Drafts", href: "/acme/drafts/" }];
    const { rerender } = render(sidebar({ personalEntries, currentPath: "/acme/" }));
    expect(screen.getByRole("link", { name: "Drafts" })).not.toHaveAttribute("aria-current");
    rerender(sidebar({ personalEntries }));
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
});
