// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-screen settings bar (WSET-001, spec §1): the hamburger opens the
// same filtered entries beside the section crumb; the panel closes on an
// outside press, on Escape, and after following an entry.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_MEMBER, selectPermissions } from "../../../core/session/index.js";
import { visibleWorkspaceSettingsGroups } from "../lib/sections.js";
import { SettingsMobileNav } from "./SettingsMobileNav.js";

function renderNav(role = ROLE_ADMIN) {
  const onNavigate = vi.fn();
  render(
    <SettingsMobileNav
      groups={visibleWorkspaceSettingsGroups("acme", selectPermissions(role))}
      activeSection="members"
      activeLabel="Members"
      onNavigate={onNavigate}
    />
  );
  return { onNavigate };
}

describe("SettingsMobileNav", () => {
  it("names the current section beside the closed hamburger", () => {
    renderNav();
    expect(screen.getByTestId("workspace-settings-mobile-crumb")).toHaveTextContent("Members");
    expect(screen.getByRole("button", { name: "Open settings navigation" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId("workspace-settings-drawer")).not.toBeInTheDocument();
  });

  it("hides the whole bar on wide viewports where the sidebar takes over", () => {
    renderNav();
    const trigger = screen.getByRole("button", { name: "Open settings navigation" });
    expect(trigger.closest("div.relative")).toHaveClass("md:hidden");
  });

  it("opens the drawer with the role-filtered entries and closes it again", () => {
    renderNav(ROLE_MEMBER);
    const trigger = screen.getByRole("button", { name: "Open settings navigation" });
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    const drawer = screen.getByTestId("workspace-settings-drawer");
    expect(drawer).toContainElement(screen.getByRole("link", { name: "General" }));
    expect(drawer).toContainElement(screen.getByRole("link", { name: "Exports" }));
    expect(screen.queryByRole("link", { name: "Webhooks" })).not.toBeInTheDocument();
    fireEvent.click(trigger);
    expect(screen.queryByTestId("workspace-settings-drawer")).not.toBeInTheDocument();
  });

  it("navigates and closes after following an entry", () => {
    const { onNavigate } = renderNav();
    fireEvent.click(screen.getByRole("button", { name: "Open settings navigation" }));
    fireEvent.click(screen.getByRole("link", { name: "Exports" }));
    expect(onNavigate).toHaveBeenCalledWith("/acme/settings/exports/");
    expect(screen.queryByTestId("workspace-settings-drawer")).not.toBeInTheDocument();
  });

  it("closes on an outside press but not inside the panel", () => {
    renderNav();
    fireEvent.click(screen.getByRole("button", { name: "Open settings navigation" }));
    fireEvent.pointerDown(screen.getByTestId("workspace-settings-drawer"));
    expect(screen.getByTestId("workspace-settings-drawer")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId("workspace-settings-drawer")).not.toBeInTheDocument();
  });

  it("closes on Escape and returns focus to the hamburger", () => {
    renderNav();
    const trigger = screen.getByRole("button", { name: "Open settings navigation" });
    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByTestId("workspace-settings-drawer"), { key: "Escape" });
    expect(screen.queryByTestId("workspace-settings-drawer")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });

  it("lets a modified entry click keep the browser's meaning", () => {
    const { onNavigate } = renderNav();
    fireEvent.click(screen.getByRole("button", { name: "Open settings navigation" }));
    fireEvent.click(screen.getByRole("link", { name: "Members" }), { metaKey: true });
    expect(onNavigate).not.toHaveBeenCalled();
    expect(screen.getByTestId("workspace-settings-drawer")).toBeInTheDocument();
  });

  it("ignores keys other than Escape", () => {
    renderNav();
    fireEvent.click(screen.getByRole("button", { name: "Open settings navigation" }));
    fireEvent.keyDown(screen.getByTestId("workspace-settings-drawer"), { key: "Enter" });
    expect(screen.getByTestId("workspace-settings-drawer")).toBeInTheDocument();
  });
});
