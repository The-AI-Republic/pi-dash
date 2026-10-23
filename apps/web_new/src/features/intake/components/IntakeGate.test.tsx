// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { IntakeGate } from "./IntakeGate.js";

describe("IntakeGate (INT-002)", () => {
  it("explains that intake is off", () => {
    render(<IntakeGate canManageFeatures onManageFeatures={() => {}} />);
    expect(screen.getByRole("heading", { name: /^Intake is paused for this project/ })).toBeInTheDocument();
  });

  it("enables the settings shortcut for project admins", () => {
    const onManageFeatures = vi.fn();
    render(<IntakeGate canManageFeatures onManageFeatures={onManageFeatures} />);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onManageFeatures).toHaveBeenCalledTimes(1);
  });

  it("keeps the shortcut present but disabled for everyone else", () => {
    const onManageFeatures = vi.fn();
    render(<IntakeGate canManageFeatures={false} onManageFeatures={onManageFeatures} />);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeDisabled();
    fireEvent.click(action);
    expect(onManageFeatures).not.toHaveBeenCalled();
  });
});
