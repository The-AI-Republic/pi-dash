// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation (INT-025): the cascade warning, the accepted
// variant that keeps the work item, confirm and cancel, and the
// locked controls while the write runs.

import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DELETE_ACCEPTED_DIALOG_TITLE, DELETE_DIALOG_TITLE, IntakeDeleteDialog } from "./IntakeDeleteDialog.js";

function setup(props: Partial<React.ComponentProps<typeof IntakeDeleteDialog>> = {}) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(
    <IntakeDeleteDialog open pending={false} accepted={false} onCancel={onCancel} onConfirm={onConfirm} {...props} />
  );
  return { onCancel, onConfirm };
}

describe("IntakeDeleteDialog", () => {
  it("warns both rows go on a request that was never accepted", () => {
    setup();
    expect(screen.getByRole("dialog", { name: DELETE_DIALOG_TITLE })).toBeInTheDocument();
    expect(screen.getByText(/work item will be permanently removed/)).toBeInTheDocument();
    expect(screen.getByText(/cannot be undone/)).toBeInTheDocument();
  });

  it("says the work item stays on an accepted request", () => {
    setup({ accepted: true });
    expect(screen.getByRole("dialog", { name: DELETE_ACCEPTED_DIALOG_TITLE })).toBeInTheDocument();
    expect(screen.getByText(/work item stays/)).toBeInTheDocument();
  });

  it("confirms the delete", async () => {
    const user = userEvent.setup();
    const h = setup();
    await user.click(screen.getByRole("button", { name: "Delete" }));
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
    expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled();
  });

  it("stays closed when shut", () => {
    setup({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
