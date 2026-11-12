// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  formatMemberCount,
  orderSwitcherWorkspaces,
  switcherRoleLabel,
  WorkspaceSwitcher,
  type SwitcherWorkspace,
} from "./WorkspaceSwitcher.js";

const WORKSPACES: SwitcherWorkspace[] = [
  { id: "ws-second", slug: "second", name: "Second", role: 15, totalMembers: 1, logoSrc: null },
  { id: "ws-acme", slug: "acme", name: "Acme", role: 20, totalMembers: 3, logoSrc: "https://cdn.example/acme.png" },
];

function setup(workspaces: SwitcherWorkspace[] = WORKSPACES, overrides: Record<string, unknown> = {}) {
  const handlers = {
    onSelectWorkspace: vi.fn(),
    onOpenSettings: vi.fn(),
    onInviteMembers: vi.fn(),
    onCreateWorkspace: vi.fn(),
    onWorkspaceInvites: vi.fn(),
    onSignOut: vi.fn(),
  };
  render(
    <WorkspaceSwitcher
      workspaces={workspaces}
      activeSlug="acme"
      creationDisabled={false}
      {...handlers}
      {...overrides}
    />
  );
  return handlers;
}

function openMenu(): void {
  fireEvent.click(screen.getByTestId("topbar-workspace-switcher"));
}

describe("orderSwitcherWorkspaces", () => {
  it("lists the active workspace first and keeps the rest in order", () => {
    expect(orderSwitcherWorkspaces(WORKSPACES, "second").map((entry) => entry.slug)).toEqual(["second", "acme"]);
    expect(orderSwitcherWorkspaces(WORKSPACES, "unknown").map((entry) => entry.slug)).toEqual(["second", "acme"]);
  });
});

describe("switcherRoleLabel", () => {
  it("lowercases known roles and falls back to guest", () => {
    expect(switcherRoleLabel(20)).toBe("admin");
    expect(switcherRoleLabel(15)).toBe("member");
    expect(switcherRoleLabel(5)).toBe("guest");
    expect(switcherRoleLabel(-1)).toBe("guest");
  });
});

describe("formatMemberCount", () => {
  it("pluralizes the workspace total", () => {
    expect(formatMemberCount(0)).toBe("0 members");
    expect(formatMemberCount(1)).toBe("1 member");
    expect(formatMemberCount(3)).toBe("3 members");
  });
});

describe("WorkspaceSwitcher", () => {
  it("triggers on the active workspace mark", () => {
    setup();
    const trigger = screen.getByTestId("topbar-workspace-switcher");
    expect(trigger).toHaveAttribute("aria-label", "Open workspace switcher");
    expect(trigger).toHaveAttribute("data-keep-open", "true");
    expect(within(trigger).getByText("Acme")).toBeInTheDocument();
    expect(trigger.querySelector("img")?.getAttribute("src")).toBe("https://cdn.example/acme.png");
  });

  it("falls back to the slug when the active workspace is unknown", () => {
    setup(WORKSPACES, { activeSlug: "missing" });
    expect(within(screen.getByTestId("topbar-workspace-switcher")).getByText("missing")).toBeInTheDocument();
    openMenu();
    expect(screen.queryByTestId("workspace-row-extras")).not.toBeInTheDocument();
  });

  it("lists workspaces active-first with roles, counts and the current mark", () => {
    setup();
    openMenu();
    const menu = screen.getByRole("menu", { name: "Open workspace switcher" });
    const rows = within(menu).getAllByRole("menuitem");
    // Two workspace rows plus the three footer shortcuts.
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("Acme"),
      expect.stringContaining("Second"),
      "Create workspace",
      "Workspace invites",
      "Sign out",
    ]);
    expect(rows[0]?.textContent).toContain("admin");
    expect(rows[0]?.textContent).toContain("3 members");
    expect(rows[0]?.textContent).toContain("(current workspace)");
    expect(rows[1]?.textContent).toContain("member");
    expect(rows[1]?.textContent).toContain("1 member");
    expect(rows[1]?.textContent).not.toContain("current workspace");
  });

  it("offers settings and invites on the active row for admins", () => {
    const handlers = setup();
    openMenu();
    const extras = screen.getByTestId("workspace-row-extras");
    fireEvent.click(within(extras).getByRole("button", { name: /Settings/ }));
    expect(handlers.onOpenSettings).toHaveBeenCalledWith("acme");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: /Invite members/ }));
    expect(handlers.onInviteMembers).toHaveBeenCalledWith("acme");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("offers settings without invites for members and nothing for guests", () => {
    const member = [{ ...WORKSPACES[1]!, role: 15 }];
    const { unmount } = render(
      <WorkspaceSwitcher
        workspaces={member}
        activeSlug="acme"
        creationDisabled={false}
        onSelectWorkspace={vi.fn()}
        onOpenSettings={vi.fn()}
        onInviteMembers={vi.fn()}
        onCreateWorkspace={vi.fn()}
        onWorkspaceInvites={vi.fn()}
        onSignOut={vi.fn()}
      />
    );
    openMenu();
    expect(screen.getByRole("button", { name: /Settings/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Invite members/ })).not.toBeInTheDocument();
    unmount();
    setup([{ ...WORKSPACES[1]!, role: 5 }]);
    openMenu();
    expect(screen.queryByTestId("workspace-row-extras")).not.toBeInTheDocument();
  });

  it("selects workspaces through the rows", () => {
    const handlers = setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: /Second/ }));
    expect(handlers.onSelectWorkspace).toHaveBeenCalledWith("second");
  });

  it("offers the footer shortcuts and hides creation while disabled", () => {
    const handlers = setup();
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Create workspace" }));
    expect(handlers.onCreateWorkspace).toHaveBeenCalledTimes(1);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Workspace invites" }));
    expect(handlers.onWorkspaceInvites).toHaveBeenCalledTimes(1);
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    expect(handlers.onSignOut).toHaveBeenCalledTimes(1);
  });

  it("hides the creation row while the instance disables it", () => {
    setup(WORKSPACES, { creationDisabled: true });
    openMenu();
    expect(screen.queryByRole("menuitem", { name: "Create workspace" })).not.toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Workspace invites" })).toBeInTheDocument();
  });
});
