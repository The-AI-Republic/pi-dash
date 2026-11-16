// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval-mode control (NEWFRONT-553, RUN-035): three modes most cautious
// first, the chosen one pressed and explained, and no changes while disabled.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { APPROVAL_MODE_COPY, APPROVAL_MODES } from "./approvalCopy.js";
import { APPROVAL_MODE_GROUP_LABEL, ChatApprovalMode } from "./ChatApprovalMode.js";

describe("ChatApprovalMode", () => {
  it("renders the modes in order with the active one pressed", () => {
    render(<ChatApprovalMode value="workspace" onChange={vi.fn()} />);
    const group = screen.getByRole("group", { name: APPROVAL_MODE_GROUP_LABEL });
    expect(group).toBeInTheDocument();
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.textContent)).toEqual(
      APPROVAL_MODES.map((mode) => APPROVAL_MODE_COPY[mode].label)
    );
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.workspace.label })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label })).toHaveAttribute(
      "title",
      APPROVAL_MODE_COPY.ask.help
    );
  });

  it("reports the mode the user picks", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ChatApprovalMode value="full_access" onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label }));
    expect(onChange).toHaveBeenCalledWith("ask");
  });

  it("disables every mode while the page is busy", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ChatApprovalMode value="ask" onChange={onChange} disabled />);
    for (const button of screen.getAllByRole("button")) expect(button).toBeDisabled();
    await user.click(screen.getByRole("button", { name: APPROVAL_MODE_COPY.full_access.label }));
    expect(onChange).not.toHaveBeenCalled();
  });
});
