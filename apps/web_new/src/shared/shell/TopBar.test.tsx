// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { platform } from "../../core/platform/index.js";
import { TopBar, type TopBarProps } from "./TopBar.js";

function renderBar(overrides: Partial<TopBarProps> = {}): void {
  render(
    <TopBar
      workspaceSlug="acme"
      section="Atlas"
      currentPath="/acme/projects/p1/issues"
      sidebarCollapsed={false}
      onToggleSidebar={() => undefined}
      unreadTotal={0}
      unreadMentions={0}
      {...overrides}
    />
  );
}

describe("TopBar composition", () => {
  it("mounts the owned controls and the slice-owned slots in order", () => {
    renderBar({
      workspaceMenu: <button type="button">Workspaces</button>,
      searchBox: <input type="search" placeholder="Search commands" />,
      helpMenu: <button type="button">Help</button>,
      accountMenu: <button type="button">Account</button>,
      sidebarCollapsed: true,
    });
    const header = screen.getByRole("banner");
    const controls = [
      screen.getByRole("button", { name: "Workspaces" }),
      screen.getByRole("button", { name: "Expand sidebar" }),
      screen.getByPlaceholderText("Search commands"),
      screen.getByRole("link", { name: "Inbox" }),
      screen.getByRole("button", { name: "Help" }),
      screen.getByRole("link", { name: "Star us on GitHub" }),
      screen.getByRole("button", { name: "Account" }),
    ];
    for (const control of controls) {
      expect(header).toContainElement(control);
    }
    for (let index = 1; index < controls.length; index += 1) {
      expect(controls[index - 1]?.compareDocumentPosition(controls[index] as Node)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING
      );
    }
  });

  it("mounts no edition badge, version reveal or plans entry (SHELL-100)", () => {
    renderBar({ accountMenu: <button type="button">Account</button>, sidebarCollapsed: true });
    expect(screen.queryByRole("button", { name: "Community" })).not.toBeInTheDocument();
    expect(screen.queryByText("Community", { exact: true })).not.toBeInTheDocument();
    expect(screen.queryByText("Pro", { exact: true })).not.toBeInTheDocument();
  });
});

describe("sidebar toggle", () => {
  it("collapses through its callback with the collapse hint while expanded", async () => {
    const user = userEvent.setup();
    const onToggleSidebar = vi.fn();
    renderBar({ onToggleSidebar });
    const toggle = screen.getByRole("button", { name: "Collapse sidebar" });
    expect(toggle).toHaveAttribute("data-testid", "sidebar-toggle");
    expect(toggle).toHaveAttribute("title", "Collapse sidebar");
    await user.click(toggle);
    expect(onToggleSidebar).toHaveBeenCalledTimes(1);
  });

  it("offers to expand while collapsed", () => {
    renderBar({ sidebarCollapsed: true });
    expect(screen.getByRole("button", { name: "Expand sidebar" })).toHaveAttribute("title", "Expand sidebar");
    expect(screen.queryByRole("button", { name: "Collapse sidebar" })).not.toBeInTheDocument();
  });

  it("hides where the sidebar is unmounted", () => {
    renderBar({ sidebarMounted: false, accountMenu: <button type="button">Account</button> });
    expect(screen.queryByTestId("sidebar-toggle")).not.toBeInTheDocument();
  });

  it("hides when the edition gate closes it", () => {
    renderBar({ sidebarToggleEnabled: false });
    expect(screen.queryByTestId("sidebar-toggle")).not.toBeInTheDocument();
  });
});

describe("inbox link", () => {
  it("links the workspace inbox with no dot on an empty inbox", () => {
    renderBar();
    const inbox = screen.getByRole("link", { name: "Inbox" });
    expect(inbox).toHaveAttribute("href", "/acme/notifications/");
    expect(inbox).not.toHaveAttribute("aria-current");
    expect(inbox.querySelector("div span")).not.toBeInTheDocument();
  });

  it("reads active on the notifications route", () => {
    renderBar({ currentPath: "/acme/notifications/" });
    expect(screen.getByRole("link", { name: "Inbox" })).toHaveAttribute("aria-current", "page");
  });

  it("dots on a positive total count", () => {
    renderBar({ unreadTotal: 3 });
    expect(screen.getByRole("link", { name: "Inbox" }).querySelector("div span")).toBeInTheDocument();
  });

  it("dots on mentions alone", () => {
    renderBar({ unreadMentions: 2 });
    expect(screen.getByRole("link", { name: "Inbox" }).querySelector("div span")).toBeInTheDocument();
  });
});

describe("account fallback", () => {
  it("stays out while the sidebar is mounted and expanded", () => {
    renderBar({ accountMenu: <button type="button">Account</button> });
    expect(screen.queryByRole("button", { name: "Account" })).not.toBeInTheDocument();
  });

  it("appears once the sidebar collapses", () => {
    renderBar({ accountMenu: <button type="button">Account</button>, sidebarCollapsed: true });
    expect(screen.getByRole("button", { name: "Account" })).toBeInTheDocument();
  });

  it("appears where the sidebar is unmounted", () => {
    renderBar({ accountMenu: <button type="button">Account</button>, sidebarMounted: false });
    expect(screen.getByRole("button", { name: "Account" })).toBeInTheDocument();
  });
});

describe("star link", () => {
  it("opens the repository externally under its accessible name", () => {
    renderBar();
    const star = screen.getByRole("link", { name: "Star us on GitHub" });
    expect(star).toHaveAttribute("href", "https://github.com/The-AI-Republic/pi-dash");
    expect(star).toHaveAttribute("target", "_blank");
    expect(star.getAttribute("rel")).toContain("noopener");
    expect(star.getAttribute("rel")).toContain("noreferrer");
  });
});

describe("TopBar platform chrome", () => {
  const hadWindow = platform.window;

  afterEach(() => {
    if (hadWindow === undefined) delete platform.window;
    else platform.window = hadWindow;
  });

  it("renders plain chrome on web", () => {
    delete platform.window;
    renderBar();
    const header = screen.getByRole("banner");
    expect(header).not.toHaveAttribute("data-tauri-drag-region");
    expect(header.className).not.toContain("select-none");
  });

  it("syncs the window title through the desktop drag region", () => {
    const setTitle = vi.fn();
    platform.window = { trafficLightInset: true, setTitle };
    renderBar();
    const header = screen.getByRole("banner");
    expect(header).toHaveAttribute("data-tauri-drag-region", "true");
    expect(header.className).toContain("pl-20");
    expect(setTitle).toHaveBeenCalledWith("Atlas — Pi Dash");
  });

  it("falls back to the product title without a section", () => {
    const setTitle = vi.fn();
    platform.window = { trafficLightInset: false, setTitle };
    renderBar({ section: "" });
    expect(screen.getByRole("banner").className).not.toContain("pl-20");
    expect(setTitle).toHaveBeenCalledWith("Pi Dash");
  });
});
