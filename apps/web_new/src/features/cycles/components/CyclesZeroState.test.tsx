// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The zero-state explains cycles to every role and gates its creation
// shortcut to members and admins (CYC-011).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CyclesZeroState } from "./CyclesZeroState.js";

describe("CyclesZeroState", () => {
  it("enables creation for members and opens the slot", () => {
    const onCreateCycle = vi.fn();
    render(<CyclesZeroState canCreate onCreateCycle={onCreateCycle} />);
    expect(screen.getByTestId("cycles-zero-state")).toHaveTextContent(/dated window that groups work items/);
    const action = screen.getByRole("button", { name: "Set your first cycle" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onCreateCycle).toHaveBeenCalledTimes(1);
  });

  it("disables creation for guests and viewers without error", () => {
    render(<CyclesZeroState canCreate={false} />);
    expect(screen.getByTestId("cycles-zero-state")).toHaveTextContent(/dated window that groups work items/);
    expect(screen.getByRole("button", { name: "Set your first cycle" })).toBeDisabled();
  });

  it("tolerates a click before the dialog slot lands", () => {
    render(<CyclesZeroState canCreate />);
    fireEvent.click(screen.getByRole("button", { name: "Set your first cycle" }));
  });
});
