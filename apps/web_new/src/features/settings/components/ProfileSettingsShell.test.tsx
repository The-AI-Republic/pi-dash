// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings shell (parity/specs/profile.md §5, PROF-010): grouped tab
// navigation, active state, back control, workspace shortcuts, and the
// plain-link fallbacks for modified clicks and back to home.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BACK_HOME_LABEL, ProfileSettingsShell, SETTINGS_TITLE, workspaceInitial } from "./ProfileSettingsShell.js";

const WORKSPACES = [
  { id: "ws-1", slug: "acme", name: "Acme" },
  { id: "ws-2", slug: "beta", name: "beta studio" },
];

function setup(activeTab: "general" | "security" | "api-tokens" = "general", selfManaged?: boolean) {
  const onNavigate = vi.fn();
  render(
    <ProfileSettingsShell
      activeTab={activeTab}
      workspaces={WORKSPACES}
      selfManaged={selfManaged}
      onNavigate={onNavigate}
    >
      <p>tab body</p>
    </ProfileSettingsShell>
  );
  return { onNavigate };
}

afterEach(() => {
  document.title = "";
  vi.restoreAllMocks();
});

describe("ProfileSettingsShell", () => {
  it("groups the nine tabs and marks the active one", () => {
    setup("security");
    expect(screen.getByRole("heading", { name: "Your Profile" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Developer" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Security" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Profile" })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("link", { name: "Personal Access Tokens" })).toHaveAttribute(
      "href",
      "/settings/profile/api-tokens/"
    );
  });

  it("hides the password tab on a cloud-managed instance (CLOUD-035)", () => {
    setup("general", false);
    expect(screen.queryByRole("link", { name: "Security" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Profile" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "AI Assistant" })).toBeInTheDocument();
  });

  it("keeps the password tab on a self-managed instance and while the answer is unknown", () => {
    setup("general", true);
    expect(screen.getByRole("link", { name: "Security" })).toBeInTheDocument();
    cleanup();
    setup("general", undefined);
    expect(screen.getByRole("link", { name: "Security" })).toBeInTheDocument();
  });

  it("carries the title, sidebar label and the tab body", () => {
    setup();
    expect(screen.getByText(SETTINGS_TITLE)).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: SETTINGS_TITLE })).toBeInTheDocument();
    expect(screen.getByText("tab body")).toBeInTheDocument();
    expect(document.title).toBe("Profile · Profile settings");
  });

  it("navigates in-app when a tab link is followed plainly", async () => {
    const { onNavigate } = setup();
    await userEvent.click(screen.getByRole("link", { name: "Notifications" }));
    expect(onNavigate).toHaveBeenCalledWith("/settings/profile/notifications/");
  });

  it("leaves modified clicks to the browser", () => {
    const { onNavigate } = setup();
    const link = screen.getByRole("link", { name: "Activity" });
    const event = new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true });
    const prevented = !link.dispatchEvent(event);
    expect(prevented).toBe(false);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("lists the workspaces with initials plus the create and invite shortcuts", async () => {
    const { onNavigate } = setup();
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(screen.getByText("beta studio")).toBeInTheDocument();
    expect(screen.getByText("A")).toBeInTheDocument();
    expect(screen.getByText("B")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Create workspace" })).toHaveAttribute("href", "/create-workspace/");
    expect(screen.getByRole("link", { name: "Workspace invites" })).toHaveAttribute("href", "/invitations/");
    await userEvent.click(screen.getByRole("link", { name: /Acme/ }));
    expect(onNavigate).toHaveBeenCalledWith("/acme/");
  });

  it("returns home through a full navigation past the router", () => {
    const assign = vi.fn();
    const original = window.location;
    Object.defineProperty(window, "location", { configurable: true, value: { ...original, assign } });
    try {
      setup();
      fireEvent.click(screen.getByRole("link", { name: BACK_HOME_LABEL }));
      expect(assign).toHaveBeenCalledWith("/");
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
    }
  });

  it("keeps a modified back click with the browser", () => {
    const assign = vi.fn();
    const original = window.location;
    Object.defineProperty(window, "location", { configurable: true, value: { ...original, assign } });
    try {
      setup();
      const link = screen.getByRole("link", { name: BACK_HOME_LABEL });
      fireEvent.click(link, { altKey: true });
      expect(assign).not.toHaveBeenCalled();
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
    }
  });

  it("derives a workspace initial and falls back for a blank name", () => {
    expect(workspaceInitial("  acme")).toBe("A");
    expect(workspaceInitial("   ")).toBe("?");
  });
});
