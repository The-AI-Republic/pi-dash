// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { COMPLETED_CYCLE_NOTE, TransferIssuesPrompt } from "./TransferIssuesPrompt.js";

describe("TransferIssuesPrompt", () => {
  it("always explains that the finished cycle is closed to edits", () => {
    render(<TransferIssuesPrompt canTransfer={false} onOpen={() => undefined} />);
    expect(screen.getByText(COMPLETED_CYCLE_NOTE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Move unfinished work" })).not.toBeInTheDocument();
  });

  it("opens the dialog from the transfer button", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(<TransferIssuesPrompt canTransfer onOpen={onOpen} />);
    await user.click(screen.getByRole("button", { name: "Move unfinished work" }));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("disables the button while a transfer is in flight", () => {
    render(<TransferIssuesPrompt canTransfer disabled onOpen={() => undefined} />);
    expect(screen.getByRole("button", { name: "Move unfinished work" })).toBeDisabled();
  });
});
