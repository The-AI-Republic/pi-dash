// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { MENU_PORTAL_ID } from "./document.js";
import { useRailState } from "./railState.js";
import { isDockItemActive, isSettingsEntryActive, WorkspaceRail } from "./WorkspaceRail.js";

const DOCK = [{ id: "projects", label: "Projects", to: "/acme/" }];

function renderRail(currentPath = "/acme/projects/p1/issues"): void {
  render(<WorkspaceRail workspaceSlug="acme" currentPath={currentPath} dockItems={DOCK} />);
}

function openMenu(x = 8, y = 400): void {
  fireEvent.contextMenu(screen.getByTestId("workspace-rail"), { clientX: x, clientY: y });
}

function menu(): HTMLElement {
  return screen.getByRole("menu", { name: "App rail options" });
}

beforeEach(() => {
  useRailState.setState({ density: "labeled", undockedByWorkspace: {} });
  globalThis.localStorage.clear();
  const host = document.createElement("div");
  host.id = MENU_PORTAL_ID;
  document.body.appendChild(host);
});

afterEach(() => {
  document.getElementById(MENU_PORTAL_ID)?.remove();
  useRailState.setState({ density: "labeled", undockedByWorkspace: {} });
  globalThis.localStorage.clear();
});

describe("rail entries", () => {
  it("renders dock entries captioned with the settings entry pinned below", () => {
    renderRail();
    const rail = screen.getByTestId("workspace-rail");
    expect(rail.className).toContain("w-15");
    const projects = screen.getByRole("link", { name: "Projects" });
    expect(projects).toHaveAttribute("href", "/acme/");
    expect(projects).toHaveAttribute("aria-current", "page");
    expect(within(rail).getByText("Projects")).toBeInTheDocument();
    const settings = screen.getByRole("link", { name: "Settings" });
    expect(settings).toHaveAttribute("href", "/acme/settings");
    expect(settings).not.toHaveAttribute("aria-current");
    expect(projects.compareDocumentPosition(settings)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("drops to icon-only width with accessible names intact", () => {
    useRailState.setState({ density: "icons" });
    renderRail();
    expect(screen.getByTestId("workspace-rail").className).toContain("w-12");
    expect(screen.queryByText("Projects")).not.toBeInTheDocument();
    expect(screen.queryByText("Settings")).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Projects" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Settings" })).toBeInTheDocument();
  });

  it("highlights the settings entry on workspace settings routes only", () => {
    renderRail("/acme/settings/general");
    expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Projects" })).not.toHaveAttribute("aria-current");
  });

  it("leaves dock entries quiet off their target", () => {
    renderRail("/acme/notifications/");
    expect(screen.getByRole("link", { name: "Projects" })).not.toHaveAttribute("aria-current");
  });
});

describe("entry active rules", () => {
  it("matches the exact target and its subtree", () => {
    expect(isDockItemActive("/acme/", "/acme/", "acme")).toBe(true);
    expect(isDockItemActive("/acme/projects/p1/issues", "/acme/", "acme")).toBe(true);
    expect(isDockItemActive("/globex/", "/acme/", "acme")).toBe(false);
    expect(isDockItemActive("/acme/drafts/x", "/acme/drafts", "acme")).toBe(true);
    expect(isDockItemActive("/acme/drafts", "/acme/drafts", "acme")).toBe(true);
    expect(isDockItemActive("/acme/drafts2", "/acme/drafts", "acme")).toBe(false);
  });

  it("excludes settings and inbox routes from dock entries", () => {
    expect(isDockItemActive("/acme/settings", "/acme/", "acme")).toBe(false);
    expect(isDockItemActive("/acme/notifications/", "/acme/", "acme")).toBe(false);
  });

  it("limits the settings highlight to workspace settings routes", () => {
    expect(isSettingsEntryActive("/acme/settings", "acme")).toBe(true);
    expect(isSettingsEntryActive("/acme/settings/general", "acme")).toBe(true);
    expect(isSettingsEntryActive("/acme/projects/p1/settings", "acme")).toBe(false);
    expect(isSettingsEntryActive("/acme/", "acme")).toBe(false);
  });
});

describe("rail context menu", () => {
  it("opens at the pointer with density checks and the undock entry", () => {
    renderRail();
    openMenu(8, 400);
    const options = menu();
    expect(options).toHaveStyle({ left: "8px", top: "400px" });
    expect(options).toHaveFocus();
    expect(document.getElementById(MENU_PORTAL_ID)).toContainElement(options);
    expect(screen.getByRole("menuitemradio", { name: "Icon only" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("menuitemradio", { name: "Icon with name" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitem", { name: "Undock App Rail" })).toBeInTheDocument();
  });

  it("applies a density choice immediately and closes", async () => {
    const user = userEvent.setup();
    renderRail();
    openMenu();
    await user.click(screen.getByRole("menuitemradio", { name: "Icon only" }));
    expect(useRailState.getState().density).toBe("icons");
    expect(screen.getByTestId("workspace-rail").className).toContain("w-12");
    expect(screen.queryByRole("menu", { name: "App rail options" })).not.toBeInTheDocument();
    expect(globalThis.localStorage.getItem("pidash:rail")).toContain('"density":"icons"');
    openMenu();
    expect(screen.getByRole("menuitemradio", { name: "Icon only" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemradio", { name: "Icon with name" })).toHaveAttribute("aria-checked", "false");
  });

  it("undocks through the menu and offers re-docking while undocked", async () => {
    const user = userEvent.setup();
    renderRail();
    openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Undock App Rail" }));
    expect(useRailState.getState().undockedByWorkspace["acme"]).toBe(true);

    openMenu();
    await user.click(screen.getByRole("menuitem", { name: "Dock App Rail" }));
    expect(useRailState.getState().undockedByWorkspace["acme"]).toBe(false);
  });

  it("closes on Escape but stays open on other keys", () => {
    renderRail();
    openMenu();
    fireEvent.keyDown(menu(), { key: "a" });
    expect(menu()).toBeInTheDocument();
    fireEvent.keyDown(menu(), { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "App rail options" })).not.toBeInTheDocument();
  });

  it("closes on outside press but not on inside press", () => {
    renderRail();
    openMenu();
    fireEvent.pointerDown(screen.getByRole("menuitemradio", { name: "Icon only" }));
    expect(menu()).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu", { name: "App rail options" })).not.toBeInTheDocument();
  });
});
