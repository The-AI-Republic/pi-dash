// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Decline confirmation (INT-021): the irreversible-action warning,
// confirm and cancel, and the locked controls while the write runs.

import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DECLINE_DIALOG_TITLE, IntakeDeclineDialog } from "./IntakeDeclineDialog.js";

function setup(props: Partial<React.ComponentProps<typeof IntakeDeclineDialog>> = {}) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(<IntakeDeclineDialog open pending={false} onCancel={onCancel} onConfirm={onConfirm} {...props} />);
  return { onCancel, onConfirm };
}

describe("IntakeDeclineDialog", () => {
  it("warns the action cannot be undone", () => {
    setup();
    expect(screen.getByRole("dialog", { name: DECLINE_DIALOG_TITLE })).toBeInTheDocument();
    expect(screen.getByText(/cannot be undone/)).toBeInTheDocument();
  });

  it("confirms the decline", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Decline" }));
    expect(h.onConfirm).toHaveBeenCalledTimes(1);
    expect(h.onCancel).not.toHaveBeenCalled();
  });

  it("treats cancel, the close control and Escape as cancel", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onCancel).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(h.onCancel).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(h.onCancel).toHaveBeenCalledTimes(3);
    expect(h.onConfirm).not.toHaveBeenCalled();
  });

  it("locks the controls while the write runs", () => {
    setup({ pending: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Decline" })).toBeDisabled();
  });

  it("stays closed when shut", () => {
    setup({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
