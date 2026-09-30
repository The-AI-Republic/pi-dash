// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createKitToastManager, showToast, ToastHost } from "./Toast";

function setup() {
  const manager = createKitToastManager();
  render(<ToastHost manager={manager} />);
  return manager;
}

describe("Toast", () => {
  it("shows a notification with title and description", async () => {
    const manager = setup();
    showToast(manager, { title: "Saved", description: "Issue updated", timeout: 0 });
    expect(await screen.findByText("Saved")).toBeInTheDocument();
    expect(screen.getByText("Issue updated")).toBeInTheDocument();
  });

  it("runs the action and dismisses", async () => {
    const user = userEvent.setup();
    const manager = setup();
    const onAction = vi.fn();
    showToast(manager, { title: "Saved", actionLabel: "Undo", onAction, timeout: 0 });
    await screen.findByText("Saved");
    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
  });

  it("hides the close button until the toast expands, then dismisses", async () => {
    const user = userEvent.setup();
    const manager = setup();
    showToast(manager, { title: "Saved", timeout: 0 });
    await screen.findByText("Saved");
    // Collapsed stack: the close control stays out of the a11y tree.
    expect(screen.queryByRole("button", { name: "Dismiss notification" })).not.toBeInTheDocument();
    await user.hover(screen.getByRole("dialog", { name: "Saved" }));
    await user.click(screen.getByRole("button", { name: "Dismiss notification" }));
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
  });
});
