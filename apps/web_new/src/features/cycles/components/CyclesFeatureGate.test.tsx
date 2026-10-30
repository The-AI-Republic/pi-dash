// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feature gate explains the off switch to every role and keeps its
// management shortcut disabled without admin standing (CYC-010).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CyclesFeatureGate } from "./CyclesFeatureGate.js";

describe("CyclesFeatureGate", () => {
  it("enables the shortcut for admins and routes on select", () => {
    const onManageFeatures = vi.fn();
    render(<CyclesFeatureGate isAdmin onManageFeatures={onManageFeatures} />);
    expect(screen.getByTestId("cycles-feature-gate")).toHaveTextContent(/not enabled for this project/);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onManageFeatures).toHaveBeenCalledTimes(1);
  });

  it("keeps the shortcut rendered but disabled for non-admins", () => {
    render(<CyclesFeatureGate isAdmin={false} onManageFeatures={vi.fn()} />);
    expect(screen.getByTestId("cycles-feature-gate")).toHaveTextContent(/not enabled for this project/);
    expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled();
  });
});
