// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

function setup() {
  return render(
    <Dialog
      trigger={<Button>Edit title</Button>}
      title="Edit title"
      description="Shown in the list"
      actions={<Button variant="primary">Save</Button>}
    >
      <p>Dialog body</p>
    </Dialog>
  );
}

describe("Dialog", () => {
  it("opens from the trigger and traps focus inside", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Edit title" }));
    const dialog = screen.getByRole("dialog", { name: "Edit title" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByText("Shown in the list")).toBeInTheDocument();
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("closes on Escape and returns focus to the trigger", async () => {
    const user = userEvent.setup();
    setup();
    const trigger = screen.getByRole("button", { name: "Edit title" });
    await user.click(trigger);
    expect(screen.getByRole("dialog", { name: "Edit title" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("closes from the close button", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Edit title" }));
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
