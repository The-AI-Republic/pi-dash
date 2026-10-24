// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useShellChrome } from "./chromeState.js";
import { UserMenu } from "./UserMenu.js";

function setup(overrides: Record<string, unknown> = {}) {
  const handlers = {
    onYourWork: vi.fn(),
    onSettings: vi.fn(),
    onDeactivateAccount: vi.fn(),
    onSignOut: vi.fn(),
  };
  render(
    <UserMenu
      displayName="Ada Lovelace"
      email="ada@example.com"
      avatarSrc={null}
      role={20}
      variant="sidebar"
      {...handlers}
      {...overrides}
    />
  );
  return handlers;
}

describe("UserMenu", () => {
  it("triggers off the full account card in the sidebar variant", () => {
    setup();
    const trigger = screen.getByTestId("sidebar-user-menu");
    expect(trigger).toHaveAttribute("data-keep-open", "true");
    // The trigger takes the flexible row width; adjacent controls keep
    // their own box instead of covering its pointer target.
    expect(trigger).toHaveClass("w-full", "min-w-0");
    expect(within(trigger).getByText("Ada Lovelace")).toBeInTheDocument();
    expect(within(trigger).getByText("ada@example.com")).toBeInTheDocument();
  });

  it("triggers off the avatar alone in the compact variant", () => {
    setup({ variant: "compact" });
    expect(screen.getByTestId("topbar-user-menu")).toHaveAttribute("aria-label", "Open user menu");
    expect(screen.queryByTestId("sidebar-user-menu")).not.toBeInTheDocument();
  });

  it("shows the plain identity block with entries and no community row", () => {
    const handlers = setup();
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    const menu = screen.getByRole("menu");
    const identity = screen.getByTestId("user-menu-identity");
    expect(menu).toContainElement(identity);
    expect(within(identity).getByText("Ada Lovelace")).toBeInTheDocument();
    expect(within(identity).getByText("ada@example.com")).toBeInTheDocument();
    expect(identity.querySelector("img")).toBeNull();
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Your work", "Settings", "Deactivate account", "Sign out"]);
    expect(screen.queryByRole("menuitem", { name: "Community" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Your work" }));
    expect(handlers.onYourWork).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("renders the avatar image when one resolves", () => {
    setup({ avatarSrc: "https://cdn.example/ada.png" });
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    expect(screen.getByTestId("user-menu-identity").querySelector("img")?.getAttribute("src")).toBe(
      "https://cdn.example/ada.png"
    );
  });

  it("hides Your work from guests while keeping settings and sign-out", () => {
    setup({ role: 5 });
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    const menu = screen.getByRole("menu");
    expect(within(menu).queryByRole("menuitem", { name: "Your work" })).not.toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Settings" })).toBeInTheDocument();
    expect(within(menu).getByRole("menuitem", { name: "Sign out" })).toBeInTheDocument();
  });

  it("omits the deactivate row without its handler", () => {
    setup({ onDeactivateAccount: undefined });
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    expect(screen.queryByRole("menuitem", { name: "Deactivate account" })).not.toBeInTheDocument();
  });

  it("routes settings, deactivate and sign-out through their callbacks", () => {
    const handlers = setup();
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(handlers.onSettings).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Deactivate account" }));
    expect(handlers.onDeactivateAccount).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    expect(handlers.onSignOut).toHaveBeenCalledTimes(1);
  });

  it("leaves sidebar peek state undisturbed in both variants", () => {
    useShellChrome.getState().setPeekOpen(true);
    const { unmount } = render(
      <UserMenu
        displayName="Ada Lovelace"
        email="ada@example.com"
        avatarSrc={null}
        role={20}
        variant="compact"
        onYourWork={vi.fn()}
        onSettings={vi.fn()}
        onSignOut={vi.fn()}
      />
    );
    fireEvent.click(screen.getByTestId("topbar-user-menu"));
    expect(useShellChrome.getState().peekOpen).toBe(true);
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(useShellChrome.getState().peekOpen).toBe(true);
    unmount();
    setup();
    fireEvent.click(screen.getByTestId("sidebar-user-menu"));
    expect(useShellChrome.getState().peekOpen).toBe(true);
    useShellChrome.getState().clearPeek();
  });
});
