// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Draft delete confirmation (parity/specs/issues.md §12, ISS-140).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { DELETE_DRAFT_BODY, DELETE_DRAFT_TITLE, DeleteDraftDialog } from "./DeleteDraftDialog.js";

describe("DeleteDraftDialog", () => {
  it("names the draft and warns the delete cannot be undone", () => {
    render(<DeleteDraftDialog open draftName="Half written" onCancel={vi.fn()} onConfirm={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: DELETE_DRAFT_TITLE });
    expect(dialog).toHaveTextContent(DELETE_DRAFT_BODY);
    expect(dialog).toHaveTextContent("Half written");
  });

  it("confirms and cancels", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(<DeleteDraftDialog open draftName="Half written" onCancel={onCancel} onConfirm={onConfirm} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("cancels on any close request", () => {
    const onCancel = vi.fn();
    render(<DeleteDraftDialog open draftName="Half written" onCancel={onCancel} onConfirm={vi.fn()} />);
    fireEvent.keyDown(screen.getByRole("dialog", { name: DELETE_DRAFT_TITLE }), { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("locks both actions while the write runs", () => {
    render(<DeleteDraftDialog open draftName="Half written" deleting onCancel={vi.fn()} onConfirm={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Deleting…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("renders nothing while closed", () => {
    render(<DeleteDraftDialog open={false} draftName="Half written" onCancel={vi.fn()} onConfirm={vi.fn()} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
