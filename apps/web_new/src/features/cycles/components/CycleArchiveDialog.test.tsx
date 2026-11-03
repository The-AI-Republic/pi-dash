// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive-cycle confirmation: the cycle-naming heading, the
// restorable body, the pending hold, and confirm/cancel wiring
// (CYC-024).

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CycleArchiveDialog, type CycleArchiveDialogProps } from "./CycleArchiveDialog.js";

function renderDialog(overrides: Partial<CycleArchiveDialogProps> = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  onConfirm: ReturnType<typeof vi.fn>;
} {
  const onOpenChange = vi.fn();
  const onConfirm = vi.fn();
  render(
    <CycleArchiveDialog
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

describe("CycleArchiveDialog", () => {
  it("heads the dialog with the cycle name and a restorable body", () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "Archive cycle Cycle 7" })).toBeInTheDocument();
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent(/restore it at any time/i);
  });

  it("confirms the write and cancels to closed", async () => {
    const user = userEvent.setup();
    const { onOpenChange, onConfirm } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Archive" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("holds the confirm button while the write is in flight", async () => {
    const user = userEvent.setup();
    const { onConfirm } = renderDialog({ pending: true });
    const confirm = screen.getByRole("button", { name: "Archiving…" });
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("renders nothing when closed", () => {
    renderDialog({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
