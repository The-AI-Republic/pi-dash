// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DOCS_URL, FORUM_URL } from "./externalLinks.js";
import { HelpMenu, SALES_EMAIL_URL } from "./HelpMenu.js";

function setup() {
  const handlers = { onOpenShortcuts: vi.fn(), onOpenUpdates: vi.fn(), onOpenExternal: vi.fn() };
  render(<HelpMenu version="1.3.0" {...handlers} />);
  return handlers;
}

describe("HelpMenu", () => {
  it("lists help entries above the version footer", () => {
    setup();
    fireEvent.click(screen.getByTestId("topbar-help-menu"));
    const menu = screen.getByRole("menu", { name: "Help" });
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent)
    ).toEqual(["Documentation", "Contact sales", "Keyboard shortcuts", "What's new?", "Forum"]);
    expect(screen.getByTestId("help-menu-version")).toHaveTextContent("Version: v1.3.0");
  });

  it("opens external entries outside the app", () => {
    const handlers = setup();
    fireEvent.click(screen.getByTestId("topbar-help-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Documentation" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith(DOCS_URL);
    fireEvent.click(screen.getByTestId("topbar-help-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Contact sales" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith(SALES_EMAIL_URL);
    fireEvent.click(screen.getByTestId("topbar-help-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Forum" }));
    expect(handlers.onOpenExternal).toHaveBeenCalledWith(FORUM_URL);
  });

  it("opens the in-place dialogs through their callbacks", () => {
    const handlers = setup();
    fireEvent.click(screen.getByTestId("topbar-help-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Keyboard shortcuts" }));
    expect(handlers.onOpenShortcuts).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("topbar-help-menu"));
    fireEvent.click(screen.getByRole("menuitem", { name: "What's new?" }));
    expect(handlers.onOpenUpdates).toHaveBeenCalledTimes(1);
  });
});
