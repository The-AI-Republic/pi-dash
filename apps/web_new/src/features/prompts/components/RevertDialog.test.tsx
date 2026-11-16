// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { RevertDialog, revertDialogBody } from "./RevertDialog.js";

describe("revertDialogBody", () => {
  it("writes the workspace copy", () => {
    const body = revertDialogBody("workspace");
    expect(body).toContain("every member");
    expect(body).toContain("can't be undone");
  });

  it("writes the personal copy", () => {
    const body = revertDialogBody("user");
    expect(body).toContain("you trigger");
    expect(body).toContain("can't be undone");
  });
});

describe("RevertDialog", () => {
  it("confirms and cancels the revert", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(<RevertDialog scope="user" open pending={false} onConfirm={onConfirm} onCancel={onCancel} />);
    expect(screen.getByRole("heading", { name: "Revert to default?" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Revert" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("holds the dialog while the revert is in flight", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(<RevertDialog scope="workspace" open pending onConfirm={onConfirm} onCancel={onCancel} />);
    expect(screen.getByRole("button", { name: "Reverting…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
  });

  it("renders nothing when closed", () => {
    render(<RevertDialog scope="user" open={false} pending={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
