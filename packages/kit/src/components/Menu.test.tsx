// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { POPOVER_LAYER_CLASS } from "../lib/styles";
import { Button } from "./Button";
import { isMenuCustom, isMenuSeparator, Menu } from "./Menu";
import { positionerOf } from "./overlays.testSupport";

function setup(onSelect: () => void = vi.fn()) {
  return render(
    <Menu
      trigger={<Button>Actions</Button>}
      items={[
        { label: "Edit", onSelect },
        { label: "Duplicate", onSelect },
        { separator: true },
        { label: "Archive", onSelect, disabled: true },
      ]}
    />
  );
}

describe("Menu", () => {
  it("opens on ArrowDown, moves with arrows, and selects with Enter", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    setup(onSelect);
    screen.getByRole("button", { name: "Actions" }).focus();
    await user.keyboard("{ArrowDown}");
    // The open menu takes its accessible name from the trigger.
    expect(screen.getByRole("menu", { name: "Actions" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Edit" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Duplicate" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("opens and selects with the mouse", () => {
    // fireEvent dispatches the real click path. user-event's back-to-back
    // pointer sequence trips Base UI's 200ms mouseup guard under jsdom, so
    // mouse behavior is covered here while keyboard behavior uses user-event.
    const onSelect = vi.fn();
    setup(onSelect);
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(screen.getByRole("menu", { name: "Actions" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("lands the menu on the popover tier above in-page chrome", () => {
    setup();
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    expect(positionerOf(screen.getByRole("menu")).className).toContain(POPOVER_LAYER_CLASS);
  });

  it("skips disabled items and closes on Escape with focus back on the trigger", async () => {
    const user = userEvent.setup();
    setup();
    const trigger = screen.getByRole("button", { name: "Actions" });
    trigger.focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: "Archive" })).toHaveAttribute("aria-disabled", "true");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("renders an action's description as its own muted line inside the item", () => {
    render(
      <Menu
        trigger={<Button>Actions</Button>}
        items={[
          { label: "Archive", description: "Finish the work item first.", onSelect: () => {}, disabled: true },
          { label: "Delete", onSelect: () => {} },
        ]}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions" }));
    const archive = screen.getByRole("menuitem", { name: /Archive/ });
    const note = archive.querySelector("[data-menu-description='true']");
    expect(note).not.toBeNull();
    expect(note?.textContent).toBe("Finish the work item first.");
    // The note stays inside the item, not beside it.
    expect(screen.getAllByText("Finish the work item first.")).toHaveLength(1);
    // The label keeps its own element, matching the heading the parity
    // driver reads a menu item's title from.
    expect(archive.querySelector("[data-menu-title='true']")?.textContent).toBe("Archive");
    expect(screen.getByRole("menuitem", { name: "Delete" })).toHaveTextContent("Delete");
  });

  it("renders leading icons and keys repeated labels by item key", () => {
    const onSelect = vi.fn();
    render(
      <Menu
        trigger={<Button>Projects</Button>}
        items={[
          { label: "Web", itemKey: "p1", icon: <i data-testid="mark-p1" />, onSelect },
          { label: "Web", itemKey: "p2", icon: <i data-testid="mark-p2" />, onSelect },
        ]}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Projects" }));
    const rows = screen.getAllByRole("menuitem", { name: "Web" });
    expect(rows).toHaveLength(2);
    expect(screen.getByTestId("mark-p1")).toBeInTheDocument();
    expect(screen.getByTestId("mark-p2")).toBeInTheDocument();
    fireEvent.click(rows[1]!);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("keeps the menu open after a select when the row opts out of closing", () => {
    // Multi-select menus (the inbox origin filter) toggle several rows in
    // one visit; those rows set closeOnClick false.
    const onSelect = vi.fn();
    render(
      <Menu
        trigger={<Button>Filter</Button>}
        items={[
          { label: "Assigned", itemKey: "assigned", closeOnClick: false, onSelect },
          { label: "Created", itemKey: "created", onSelect },
        ]}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Filter" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Assigned" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("menu", { name: "Filter" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Created" }));
    expect(onSelect).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("renders custom entries as direct popup children with no menuitem wrapper", () => {
    const onSelect = vi.fn();
    render(
      <Menu
        trigger={<Button>Account</Button>}
        items={[
          { custom: true, entryKey: "identity", node: <span data-testid="identity">admin@example.com</span> },
          { label: "Switch to dark mode", onSelect },
          {
            custom: true,
            entryKey: "sign-out",
            node: (
              <form data-testid="sign-out-form" action="/sign-out/" method="post">
                <button type="submit" role="menuitem">
                  Sign out
                </button>
              </form>
            ),
          },
        ]}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Account" }));
    const menu = screen.getByRole("menu", { name: "Account" });
    // The identity row and the form are the popup's own children.
    expect(menu).toContainElement(screen.getByTestId("identity"));
    expect(menu).toContainElement(screen.getByTestId("sign-out-form"));
    expect(screen.getByTestId("identity").parentElement).toBe(menu);
    expect(screen.getByTestId("sign-out-form").parentElement).toBe(menu);
    // Actions still behave, and the nested menuitem matches exactly once.
    expect(screen.getAllByRole("menuitem", { name: "Sign out" })).toHaveLength(1);
    fireEvent.click(screen.getByRole("menuitem", { name: "Switch to dark mode" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("renders rich action content in place of the label with menuitem behavior", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <Menu
        trigger={<Button>Workspaces</Button>}
        items={[
          {
            label: "Acme",
            onSelect,
            content: (
              <span data-testid="rich-row">
                <strong>Acme</strong> admin
              </span>
            ),
          },
          { label: "Second", onSelect },
        ]}
      />
    );
    screen.getByRole("button", { name: "Workspaces" }).focus();
    await user.keyboard("{ArrowDown}");
    const row = screen.getByTestId("rich-row");
    expect(row).toBeInTheDocument();
    // The rich row is the menuitem itself: arrow-key focus lands on it
    // and Enter activates it.
    const item = screen.getByRole("menuitem", { name: "Acme admin" });
    expect(item).toContainElement(row);
    expect(item).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("classifies entries by shape", () => {
    expect(isMenuCustom({ custom: true, entryKey: "x", node: null })).toBe(true);
    expect(isMenuCustom({ label: "Edit", onSelect: () => {} })).toBe(false);
    expect(isMenuCustom({ separator: true })).toBe(false);
    expect(isMenuSeparator({ separator: true })).toBe(true);
    expect(isMenuSeparator({ custom: true, entryKey: "x", node: null })).toBe(false);
  });
});
