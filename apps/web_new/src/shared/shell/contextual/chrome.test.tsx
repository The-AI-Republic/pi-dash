// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ContextChip } from "./chip.js";
import { DeleteWorkItemDialog } from "./deleteDialog.js";

describe("ContextChip", () => {
  it("names the subject and clears on request", async () => {
    const user = userEvent.setup();
    const onClear = vi.fn();
    render(<ContextChip indicator="Work item" subject="ACME-12" onClear={onClear} />);
    expect(screen.getByText("Work item")).toBeInTheDocument();
    expect(screen.getByText("ACME-12")).toBeInTheDocument();
    const clear = screen.getByRole("button", { name: "Clear context (Backspace)" });
    expect(clear).toHaveAttribute("tabIndex", "-1");
    await user.click(clear);
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});

describe("DeleteWorkItemDialog", () => {
  it("confirms and cancels the delete", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const onConfirm = vi.fn();
    render(
      <DeleteWorkItemDialog
        open
        onOpenChange={onOpenChange}
        noun="Work item"
        title="Fix the leak"
        deleting={false}
        onConfirm={onConfirm}
      />
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(/“Fix the leak” will be permanently removed/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("names epics and disables the confirm while deleting", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(
      <DeleteWorkItemDialog
        open
        onOpenChange={() => undefined}
        noun="Epic"
        title="Big one"
        deleting
        onConfirm={onConfirm}
      />
    );
    expect(screen.getByText("Delete epic?")).toBeInTheDocument();
    const confirm = screen.getByRole("button", { name: "Delete" });
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
