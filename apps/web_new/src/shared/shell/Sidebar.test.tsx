// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as React from "react";
import { describe, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar.js";

const WORKSPACES = [
  { id: "w1", slug: "acme", name: "Acme" },
  { id: "w2", slug: "globex", name: "Globex" },
];

const PROJECTS = [
  { id: "p1", name: "Web", identifier: "WEB" },
  { id: "p2", name: "Mobile", identifier: "MOB" },
];

const PERSONAL = [{ key: "drafts", label: "Drafts", href: "/acme/drafts/" }];

function sidebar(props: Partial<React.ComponentProps<typeof Sidebar>> = {}): React.ReactElement {
  return (
    <Sidebar
      workspaces={WORKSPACES}
      activeSlug="acme"
      onSelectWorkspace={() => undefined}
      projects={PROJECTS}
      activeProjectId="p1"
      onSelectProject={() => undefined}
      userLabel="ada@example.com"
      onSignOut={() => undefined}
      {...props}
    />
  );
}

describe("Sidebar", () => {
  it("shows the active workspace and its projects", () => {
    render(sidebar());
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(screen.getByText("Web")).toBeInTheDocument();
    expect(screen.getByText("Mobile")).toBeInTheDocument();
    expect(screen.getByText("Web").closest("button")).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
  });

  it("falls back to the slug for an unknown workspace", () => {
    render(sidebar({ activeSlug: "unknown" }));
    expect(screen.getByRole("button", { name: "Switch workspace, current: unknown" })).toBeInTheDocument();
  });

  it("selects workspaces and projects through callbacks", async () => {
    const user = userEvent.setup();
    const onSelectWorkspace = vi.fn();
    const onSelectProject = vi.fn();
    render(sidebar({ activeProjectId: null, onSelectWorkspace, onSelectProject }));
    await user.click(screen.getByRole("button", { name: /Switch workspace/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Globex" }));
    expect(onSelectWorkspace).toHaveBeenCalledWith("globex");
    await user.click(screen.getByText("Mobile"));
    expect(onSelectProject).toHaveBeenCalledWith("p2");
  });

  it("names the empty project list", () => {
    render(sidebar({ projects: [], activeProjectId: null }));
    expect(screen.getByText("No projects yet.")).toBeInTheDocument();
  });

  it("omits deactivation from the account menu without a handler", async () => {
    const user = userEvent.setup();
    render(sidebar());
    await user.click(screen.getByRole("button", { name: "Account actions" }));
    expect(await screen.findByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Deactivate account" })).not.toBeInTheDocument();
  });

  it("fires the deactivation handler from the account menu", async () => {
    const user = userEvent.setup();
    const onDeactivateAccount = vi.fn();
    render(sidebar({ onDeactivateAccount }));
    await user.click(screen.getByRole("button", { name: "Account actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Deactivate account" }));
    expect(onDeactivateAccount).toHaveBeenCalledTimes(1);
  });

  it("mounts the navigation landmark under the stable sidebar id", () => {
    const { container } = render(sidebar({ projects: [], activeProjectId: null }));
    const landmark = screen.getByRole("complementary", { name: "Workspace navigation" });
    // The parity drivers and the shell contract locate the sidebar by this id.
    expect(container.querySelector("#main-sidebar")).toBe(landmark);
  });

  it("renders the desktop update mount beside the account area when provided", () => {
    render(
      sidebar({ projects: [], activeProjectId: null, updateControl: <button type="button">Install update</button> })
    );
    expect(screen.getByRole("button", { name: "Install update" })).toBeInTheDocument();
  });

  it("mounts no update control on web builds (SHELL-102)", () => {
    render(sidebar({ projects: [], activeProjectId: null }));
    expect(screen.queryByRole("button", { name: /update/i })).not.toBeInTheDocument();
  });

  it("heads the shell with the brand and the personalization launcher", async () => {
    const user = userEvent.setup();
    const onPersonalize = vi.fn();
    render(sidebar({ onPersonalize }));
    expect(screen.getByTestId("sidebar-brand")).toHaveTextContent("Pi Dash");
    await user.click(screen.getByTestId("personalize-nav"));
    expect(onPersonalize).toHaveBeenCalledTimes(1);
  });

  it("mounts no launcher without a personalization handler", () => {
    render(sidebar());
    expect(screen.getByTestId("sidebar-brand")).toBeInTheDocument();
    expect(screen.queryByTestId("personalize-nav")).not.toBeInTheDocument();
  });

  it("renders provided quick actions in their slot", () => {
    render(sidebar({ quickActions: <button type="button">New work item</button> }));
    expect(screen.getByTestId("sidebar-quick-actions")).toHaveTextContent("New work item");
  });

  it("lists personal entries above projects with the current one marked", () => {
    render(sidebar({ personalEntries: PERSONAL, currentPath: "/acme/drafts/" }));
    const drafts = screen.getByRole("link", { name: "Drafts" });
    expect(drafts).toHaveAttribute("href", "/acme/drafts/");
    expect(drafts).toHaveAttribute("aria-current", "page");
  });

  it("leaves personal entries unmarked off their address or without a path", () => {
    const { rerender } = render(sidebar({ personalEntries: PERSONAL, currentPath: "/acme/" }));
    expect(screen.getByRole("link", { name: "Drafts" })).not.toHaveAttribute("aria-current");
    rerender(sidebar({ personalEntries: PERSONAL }));
    expect(screen.getByRole("link", { name: "Drafts" })).not.toHaveAttribute("aria-current");
  });

  it("renders no personal section without entries", () => {
    render(sidebar());
    expect(screen.queryByRole("navigation", { name: "Personal" })).not.toBeInTheDocument();
  });
});
