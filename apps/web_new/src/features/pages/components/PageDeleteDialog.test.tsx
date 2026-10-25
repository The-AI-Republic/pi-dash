// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation (parity/specs/pages.md §3.8, PAGE-016): the body
// names the page, warns that deletion is permanent and cannot be undone,
// the confirm control holds a busy state while the write flies, and
// cancelling only closes.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PageDeleteDialog, type PageDeleteDialogProps } from "./PageDeleteDialog.js";

function renderDialog(overrides: Partial<PageDeleteDialogProps> = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  onConfirm: ReturnType<typeof vi.fn>;
} {
  const onOpenChange = vi.fn();
  const onConfirm = vi.fn();
  render(
    <PageDeleteDialog
      open
      onOpenChange={onOpenChange}
      pageName="Release checklist"
      pending={false}
      onConfirm={onConfirm}
      {...overrides}
    />
  );
  return { onOpenChange, onConfirm };
}

describe("PageDeleteDialog", () => {
  it("heads the dialog Delete page and names the page in a permanence warning", () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "Delete page" })).toBeInTheDocument();
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Release checklist");
    expect(dialog).toHaveTextContent(/permanently/i);
    expect(dialog).toHaveTextContent(/cannot be undone/i);
  });

  it("names an untitled page with the fallback label", () => {
    renderDialog({ pageName: "   " });
    expect(screen.getByRole("dialog")).toHaveTextContent("Untitled");
  });

  it("confirms the write and cancels to closed", async () => {
    const user = userEvent.setup();
    const { onOpenChange, onConfirm } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("holds both controls while the write is in flight", async () => {
    const user = userEvent.setup();
    const { onOpenChange, onConfirm } = renderDialog({ pending: true });
    const confirm = screen.getByRole("button", { name: "Deleting…" });
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(cancel).toBeDisabled();
    await user.click(cancel);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("dismisses on Escape when no write is running", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("refuses to dismiss while the request is in flight", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog({ pending: true });
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("keeps the dialog mounted when closed", () => {
    renderDialog({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
