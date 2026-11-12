// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete-cycle confirmation: the heading, the cycle-naming body, the
// pending hold, and confirm/cancel wiring (CYC-022).

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CycleDeleteDialog, type CycleDeleteDialogProps } from "./CycleDeleteDialog.js";

function renderDialog(overrides: Partial<CycleDeleteDialogProps> = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  onConfirm: ReturnType<typeof vi.fn>;
} {
  const onOpenChange = vi.fn();
  const onConfirm = vi.fn();
  render(
    <CycleDeleteDialog
      open
      onOpenChange={onOpenChange}
      cycleName="Cycle 7"
      pending={false}
      onConfirm={onConfirm}
      {...overrides}
    />
  );
  return { onOpenChange, onConfirm };
}

describe("CycleDeleteDialog", () => {
  it("heads the dialog Delete cycle and names the cycle in a permanent body", () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "Delete cycle" })).toBeInTheDocument();
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Cycle 7");
    expect(dialog).toHaveTextContent(/permanently deleted/i);
  });

  it("confirms the write and cancels to closed", async () => {
    const user = userEvent.setup();
    const { onOpenChange, onConfirm } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("holds the confirm button while the write is in flight", async () => {
    const user = userEvent.setup();
    const { onConfirm } = renderDialog({ pending: true });
    const confirm = screen.getByRole("button", { name: "Deleting…" });
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("renders nothing when closed", () => {
    renderDialog({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
