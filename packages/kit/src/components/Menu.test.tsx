// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "./Button";
import { Menu } from "./Menu";

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
});
