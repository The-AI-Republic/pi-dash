// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Danger zone (parity/specs/profile.md §6, PROF-014): the section warns,
// the dialog confirms, cancel keeps the account.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DEACTIVATE_DESCRIPTION, DeactivateAccountSection } from "./DeactivateAccountSection.js";

describe("DeactivateAccountSection", () => {
  it("explains what deactivation removes", () => {
    render(<DeactivateAccountSection confirming={false} onConfirm={vi.fn()} />);
    expect(screen.getByRole("heading", { name: "Deactivate account" })).toBeInTheDocument();
    expect(screen.getByText(DEACTIVATE_DESCRIPTION)).toHaveTextContent("permanently removed");
  });

  it("asks for confirmation before deleting", async () => {
    const onConfirm = vi.fn();
    render(<DeactivateAccountSection confirming={false} onConfirm={onConfirm} />);
    await userEvent.click(screen.getByRole("button", { name: "Deactivate account" }));
    const dialog = screen.getByRole("dialog", { name: "Deactivate your account" });
    expect(dialog).toHaveTextContent("This removes the account permanently.");
    expect(onConfirm).not.toHaveBeenCalled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Deactivate account" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("keeps the account when the dialog is cancelled", async () => {
    const onConfirm = vi.fn();
    render(<DeactivateAccountSection confirming={false} onConfirm={onConfirm} />);
    await userEvent.click(screen.getByRole("button", { name: "Deactivate account" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("locks the dialog while the deletion runs", async () => {
    const onConfirm = vi.fn();
    const view = render(<DeactivateAccountSection confirming={false} onConfirm={onConfirm} />);
    await userEvent.click(screen.getByRole("button", { name: "Deactivate account" }));
    view.rerender(<DeactivateAccountSection confirming onConfirm={onConfirm} />);
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());
  });
});
