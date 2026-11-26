// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval-mode control (NEWFRONT-553, RUN-035): a dropdown of the three
// modes most cautious first, the chosen one selected and explained, and no
// changes while disabled.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { APPROVAL_MODE_COPY, APPROVAL_MODES } from "./approvalCopy.js";
import { APPROVAL_MODE_LABEL, APPROVAL_MODE_NEXT_THREAD_NOTE, ChatApprovalMode } from "./ChatApprovalMode.js";

describe("ChatApprovalMode", () => {
  it("lists the modes in order with the active one selected and explained", () => {
    render(<ChatApprovalMode value="workspace" onChange={vi.fn()} />);
    const select = screen.getByRole("combobox", { name: APPROVAL_MODE_LABEL });
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(
      APPROVAL_MODES.map((mode) => APPROVAL_MODE_COPY[mode].label)
    );
    expect(select).toHaveValue("workspace");
    expect(select).toHaveAttribute("title", `${APPROVAL_MODE_COPY.workspace.help} ${APPROVAL_MODE_NEXT_THREAD_NOTE}`);
  });

  it("reports the mode the user picks", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ChatApprovalMode value="full_access" onChange={onChange} />);
    await user.selectOptions(screen.getByRole("combobox", { name: APPROVAL_MODE_LABEL }), "ask");
    expect(onChange).toHaveBeenCalledWith("ask");
  });

  it("is disabled while the page is busy", () => {
    render(<ChatApprovalMode value="ask" onChange={vi.fn()} disabled />);
    expect(screen.getByRole("combobox", { name: APPROVAL_MODE_LABEL })).toBeDisabled();
  });
});
