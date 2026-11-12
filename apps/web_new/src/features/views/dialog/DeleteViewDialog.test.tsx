// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DELETE_VIEW_BODY, DELETE_VIEW_TITLE, DeleteViewDialog } from "./DeleteViewDialog.js";

describe("DeleteViewDialog", () => {
  it("warns about permanent loss and confirms or cancels", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(<DeleteViewDialog open={true} onCancel={onCancel} onConfirm={onConfirm} />);

    expect(screen.getByRole("heading", { name: DELETE_VIEW_TITLE })).toBeInTheDocument();
    expect(screen.getByText(DELETE_VIEW_BODY, { exact: false })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("pins the actions while the write flies", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(<DeleteViewDialog open={true} deleting={true} onCancel={onCancel} onConfirm={onConfirm} />);

    expect(screen.getByRole("button", { name: "Deleting…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("renders nothing while closed", () => {
    render(<DeleteViewDialog open={false} onCancel={() => {}} onConfirm={() => {}} />);
    expect(screen.queryByRole("heading", { name: DELETE_VIEW_TITLE })).not.toBeInTheDocument();
  });
});
