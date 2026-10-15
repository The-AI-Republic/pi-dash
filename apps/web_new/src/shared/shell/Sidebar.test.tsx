// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

describe("Sidebar", () => {
  it("shows the active workspace and its projects", () => {
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={PROJECTS}
        activeProjectId="p1"
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(screen.getByText("Web")).toBeInTheDocument();
    expect(screen.getByText("Mobile")).toBeInTheDocument();
    expect(screen.getByText("Web").closest("button")).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
  });

  it("selects workspaces and projects through callbacks", async () => {
    const user = userEvent.setup();
    const onSelectWorkspace = vi.fn();
    const onSelectProject = vi.fn();
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={onSelectWorkspace}
        projects={PROJECTS}
        activeProjectId={null}
        onSelectProject={onSelectProject}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    await user.click(screen.getByRole("button", { name: /Switch workspace/ }));
    await user.click(await screen.findByRole("menuitem", { name: "Globex" }));
    expect(onSelectWorkspace).toHaveBeenCalledWith("globex");
    await user.click(screen.getByText("Mobile"));
    expect(onSelectProject).toHaveBeenCalledWith("p2");
  });

  it("names the empty project list", () => {
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={[]}
        activeProjectId={null}
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    expect(screen.getByText("No projects yet.")).toBeInTheDocument();
  });

  it("omits deactivation from the account menu without a handler", async () => {
    const user = userEvent.setup();
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={PROJECTS}
        activeProjectId="p1"
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    await user.click(screen.getByRole("button", { name: "Account actions" }));
    expect(await screen.findByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Deactivate account" })).not.toBeInTheDocument();
  });

  it("fires the deactivation handler from the account menu", async () => {
    const user = userEvent.setup();
    const onDeactivateAccount = vi.fn();
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={PROJECTS}
        activeProjectId="p1"
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
        onDeactivateAccount={onDeactivateAccount}
      />
    );
    await user.click(screen.getByRole("button", { name: "Account actions" }));
    await user.click(await screen.findByRole("menuitem", { name: "Deactivate account" }));
    expect(onDeactivateAccount).toHaveBeenCalledTimes(1);
  });

  it("mounts the navigation landmark under the stable sidebar id", () => {
    const { container } = render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={[]}
        activeProjectId={null}
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    const landmark = screen.getByRole("complementary", { name: "Workspace navigation" });
    // The parity drivers and the shell contract locate the sidebar by this id.
    expect(container.querySelector("#main-sidebar")).toBe(landmark);
  });

  it("collapses to an inert zero-width strip that keeps the landmark mounted", () => {
    const { container } = render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={PROJECTS}
        activeProjectId={null}
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
        collapsed
      />
    );
    const landmark = container.querySelector("#main-sidebar");
    expect(landmark?.className).toContain("w-0");
    expect(landmark?.className).toContain("overflow-hidden");
    expect(landmark?.className).not.toContain("w-60");
    expect(landmark).toHaveAttribute("inert");
  });

  it("expands interactive by default", () => {
    const { container } = render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={PROJECTS}
        activeProjectId={null}
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    const landmark = container.querySelector("#main-sidebar");
    expect(landmark?.className).toContain("w-60");
    expect(landmark).not.toHaveAttribute("inert");
  });

  it("renders the desktop update mount beside the account area when provided", () => {
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={[]}
        activeProjectId={null}
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
        updateControl={<button type="button">Install update</button>}
      />
    );
    expect(screen.getByRole("button", { name: "Install update" })).toBeInTheDocument();
  });

  it("mounts no update control on web builds (SHELL-102)", () => {
    render(
      <Sidebar
        workspaces={WORKSPACES}
        activeSlug="acme"
        onSelectWorkspace={() => undefined}
        projects={[]}
        activeProjectId={null}
        onSelectProject={() => undefined}
        userLabel="ada@example.com"
        onSignOut={() => undefined}
      />
    );
    expect(screen.queryByRole("button", { name: /update/i })).not.toBeInTheDocument();
  });
});
