// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { StateDeleteDialog } from "./StateDeleteDialog.js";

afterEach(() => {
  cleanup();
});

function renderDialog(props: Partial<React.ComponentProps<typeof StateDeleteDialog>> = {}) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(<StateDeleteDialog stateName={null} pending={false} onCancel={onCancel} onConfirm={onConfirm} {...props} />);
  return { onCancel, onConfirm };
}

describe("StateDeleteDialog", () => {
  it("stays closed with no state awaiting confirmation", () => {
    renderDialog();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("names the state and warns the removal is permanent", async () => {
    renderDialog({ stateName: "Doomed" });
    const dialog = await screen.findByRole("dialog");
    expect(screen.getByTestId("state-delete-message")).toHaveTextContent("Doomed");
    expect(dialog).toHaveTextContent(/permanently/i);
    expect(dialog).toHaveTextContent(/cannot be undone/i);
  });

  it("confirms through the danger action", async () => {
    const user = userEvent.setup();
    const { onConfirm } = renderDialog({ stateName: "Doomed" });
    await user.click(await screen.findByRole("button", { name: "Delete state" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("cancels from the button and disables both actions while pending", async () => {
    const user = userEvent.setup();
    const { onCancel } = renderDialog({ stateName: "Doomed", pending: true });
    const cancel = await screen.findByRole("button", { name: "Cancel" });
    expect(cancel).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete state" })).toBeDisabled();
    await user.click(cancel);
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("reports a cancel through the open-state change", async () => {
    const user = userEvent.setup();
    const { onCancel } = renderDialog({ stateName: "Doomed" });
    await user.click(await screen.findByRole("button", { name: "Close dialog" }));
    expect(onCancel).toHaveBeenCalled();
  });
});
