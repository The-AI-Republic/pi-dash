// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account settings navigator (PROF-009/010; main#670/#675): grouped tabs
// with the addressed one current, a back control to the workspace home,
// the Workspace section, and the narrow-screen drawer that opens from a
// hamburger and closes on Escape, an outside press, or a pick.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../../core/session/index.js";
import type { ProfileSettingsTabId } from "../../lib/tabNavigation.js";
import {
  AccountSettingsNavigator,
  ACCOUNT_SETTINGS_BACK_LABEL,
  ACCOUNT_SETTINGS_NAV_LABEL,
  ACCOUNT_SETTINGS_TITLE,
} from "./AccountSettingsNavigator.js";

const WORKSPACES = [
  { id: "ws-1", slug: "acme", name: "Acme", role: ROLE_MEMBER },
  { id: "ws-2", slug: "globex", name: "globex studio", role: ROLE_ADMIN },
];

function setup({
  activeTab = "general" as const,
  activeLabel = "Profile",
  pathname = "/acme/settings/account/general/",
  served = () => true,
}: {
  activeTab?: "general" | "security" | "api-tokens";
  activeLabel?: string | null;
  pathname?: string;
  served?: (tab: ProfileSettingsTabId) => boolean;
} = {}) {
  const onNavigate = vi.fn();
  render(
    <AccountSettingsNavigator
      workspaceSlug="acme"
      activeTab={activeTab}
      workspaces={WORKSPACES}
      activeLabel={activeLabel}
      pathname={pathname}
      onNavigate={onNavigate}
      isTabServed={served}
      tabHref={(tab) => `/acme/settings/account/${tab}/`}
    />
  );
  return { onNavigate };
}

describe("AccountSettingsNavigator", () => {
  it("groups the tabs, marks the addressed one, and addresses every link in the workspace", () => {
    setup({ activeTab: "security" });
    expect(screen.getByRole("heading", { name: "Your Profile" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Developer" })).toBeInTheDocument();
    expect(screen.getByTestId("profile-settings-tab-security")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("profile-settings-tab-general")).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("profile-settings-tab-api-tokens")).toHaveAttribute(
      "href",
      "/acme/settings/account/api-tokens/"
    );
  });

  it("carries the title and a back control to the workspace home", () => {
    const { onNavigate } = setup();
    expect(screen.getByText(ACCOUNT_SETTINGS_TITLE)).toBeInTheDocument();
    const back = screen.getByLabelText(ACCOUNT_SETTINGS_BACK_LABEL);
    expect(back).toHaveAttribute("href", "/acme");
    fireEvent.click(back);
    expect(onNavigate).toHaveBeenCalledWith("/acme");
  });

  it("leaves a modified back click to the browser", () => {
    const { onNavigate } = setup();
    const event = new MouseEvent("click", { bubbles: true, cancelable: true, metaKey: true });
    event.preventDefault();
    screen.getByLabelText(ACCOUNT_SETTINGS_BACK_LABEL).dispatchEvent(event);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("navigates in-app when a tab is followed plainly", () => {
    const { onNavigate } = setup();
    fireEvent.click(screen.getByTestId("profile-settings-tab-notifications"));
    expect(onNavigate).toHaveBeenCalledWith("/acme/settings/account/notifications/");
  });

  it("withholds a tab the instance does not serve", () => {
    setup({ served: (tab) => tab !== "security" });
    expect(screen.queryByTestId("profile-settings-tab-security")).not.toBeInTheDocument();
    expect(screen.getByTestId("profile-settings-tab-general")).toBeInTheDocument();
  });

  it("lists the workspaces plus the create and invite shortcuts, open on the current one", () => {
    setup();
    expect(screen.getByTestId("account-settings-workspace-toggle-acme")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("account-settings-workspace-toggle-globex")).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("link", { name: "Create workspace" })).toHaveAttribute("href", "/create-workspace/");
    expect(screen.getByRole("link", { name: "Workspace invites" })).toHaveAttribute("href", "/invitations/");
  });

  it("opens the drawer from the hamburger and closes it on Escape", () => {
    setup();
    expect(screen.queryByTestId("account-settings-drawer")).not.toBeInTheDocument();
    const trigger = screen.getByTestId("account-settings-menu");
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("account-settings-drawer")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId("account-settings-drawer"), { key: "Escape" });
    expect(screen.queryByTestId("account-settings-drawer")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("keeps the drawer open on other keys and closes it on an outside press", () => {
    setup();
    fireEvent.click(screen.getByTestId("account-settings-menu"));
    const drawer = screen.getByTestId("account-settings-drawer");
    fireEvent.keyDown(drawer, { key: "ArrowDown" });
    expect(screen.getByTestId("account-settings-drawer")).toBeInTheDocument();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByTestId("account-settings-drawer")).not.toBeInTheDocument();
  });

  it("keeps the drawer while the press lands inside it", () => {
    setup();
    fireEvent.click(screen.getByTestId("account-settings-menu"));
    fireEvent.pointerDown(screen.getByTestId("account-settings-drawer"));
    expect(screen.getByTestId("account-settings-drawer")).toBeInTheDocument();
  });

  it("closes the drawer after a pick and names the tab in the narrow crumb", () => {
    const { onNavigate } = setup({ activeTab: "security", activeLabel: "Security" });
    fireEvent.click(screen.getByTestId("account-settings-menu"));
    expect(screen.getByTestId("account-settings-mobile-crumb")).toHaveTextContent("Security");
    // The drawer carries its own copy of the list, so the pick is scoped
    // to the open drawer rather than the docked panel.
    fireEvent.click(
      within(screen.getByTestId("account-settings-drawer")).getByTestId("profile-settings-tab-notifications")
    );
    expect(onNavigate).toHaveBeenCalledWith("/acme/settings/account/notifications/");
    expect(screen.queryByTestId("account-settings-drawer")).not.toBeInTheDocument();
  });

  it("navigates in-app from the create and invite shortcuts and leaves modified presses to the browser", () => {
    const { onNavigate } = setup();
    fireEvent.click(screen.getByRole("link", { name: "Create workspace" }));
    expect(onNavigate).toHaveBeenLastCalledWith("/create-workspace/");
    fireEvent.click(screen.getByRole("link", { name: "Workspace invites" }));
    expect(onNavigate).toHaveBeenLastCalledWith("/invitations/");
    const modified = new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true });
    modified.preventDefault();
    screen.getByRole("link", { name: "Create workspace" }).dispatchEvent(modified);
    screen.getByRole("link", { name: "Workspace invites" }).dispatchEvent(modified);
    expect(onNavigate).toHaveBeenCalledTimes(2);
  });

  it("falls back to the settings title in the crumb while no tab is named", () => {
    setup({ activeLabel: null });
    expect(screen.getByTestId("account-settings-mobile-crumb")).toHaveTextContent(ACCOUNT_SETTINGS_TITLE);
  });

  it("moves the open row to the newly viewed workspace when the address changes in place", () => {
    // A param change keeps component state, so the row keys carry the
    // viewed flag: the destination row must arrive already open.
    const onNavigate = vi.fn();
    const props = {
      activeTab: "general" as const,
      activeLabel: "Profile",
      workspaces: WORKSPACES,
      pathname: "/acme/settings/account/general/",
      onNavigate,
      isTabServed: () => true,
      tabHref: (tab: ProfileSettingsTabId) => `/acme/settings/account/${tab}/`,
    };
    const { rerender } = render(<AccountSettingsNavigator workspaceSlug="acme" {...props} />);
    fireEvent.click(screen.getByTestId("account-settings-workspace-toggle-globex"));
    expect(screen.getByTestId("account-settings-workspace-toggle-globex")).toHaveAttribute("aria-expanded", "true");
    rerender(<AccountSettingsNavigator workspaceSlug="globex" {...props} />);
    expect(screen.getByTestId("account-settings-workspace-toggle-globex")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("account-settings-workspace-toggle-acme")).toHaveAttribute("aria-expanded", "false");
  });

  it("names the wide panel for assistive technology", () => {
    setup();
    expect(screen.getByRole("complementary", { name: ACCOUNT_SETTINGS_NAV_LABEL })).toBeInTheDocument();
  });

  it("keeps every element id unique across the docked panel and the drawer", () => {
    setup();
    fireEvent.click(screen.getByTestId("account-settings-menu"));
    const ids = Array.from(document.querySelectorAll("[id]")).map((element) => element.id);
    expect(new Set(ids).size).toBe(ids.length);
    const drawer = screen.getByTestId("account-settings-drawer");
    expect(within(drawer).getByRole("heading", { name: "Your Profile" })).toBeInTheDocument();
    expect(document.getElementById("account-settings-drawer-group-personal")).toBe(
      within(drawer).getByRole("heading", { name: "Your Profile" })
    );
  });

  it("namespaces the workspace rows' test ids per copy", () => {
    setup();
    fireEvent.click(screen.getByTestId("account-settings-menu"));
    const drawer = screen.getByTestId("account-settings-drawer");
    // The docked copy keeps the plain ids; the drawer copy must not repeat
    // them, or a row query would hit whichever copy came first.
    expect(screen.getAllByTestId("account-settings-workspace-toggle-acme")).toHaveLength(1);
    expect(within(drawer).getByTestId("account-settings-drawer-workspace-toggle-acme")).toBeInTheDocument();
    expect(within(drawer).queryByTestId("account-settings-workspace-toggle-acme")).toBeNull();
    expect(within(drawer).getByTestId("account-settings-drawer-workspace-link-acme-general")).toHaveAttribute(
      "href",
      "/acme/settings/"
    );
  });
});
