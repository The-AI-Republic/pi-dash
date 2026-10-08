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
});
