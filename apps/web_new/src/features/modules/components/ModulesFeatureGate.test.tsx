// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The flag gate (MOD-002): art, an explanation that says the feature is
// not enabled, and a Manage-features shortcut that every role sees and
// only an admin can press.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModulesFeatureGate } from "./ModulesFeatureGate.js";

describe("ModulesFeatureGate", () => {
  it("shows art, the explanation and an enabled shortcut for admins", () => {
    const onManageFeatures = vi.fn();
    const { container } = render(<ModulesFeatureGate isAdmin onManageFeatures={onManageFeatures} />);
    expect(screen.getByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(container.querySelector("img")).not.toBeNull();
    expect(screen.getByTestId("modules-feature-gate")).toHaveTextContent(/not enabled/);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onManageFeatures).toHaveBeenCalledTimes(1);
  });

  it("keeps the shortcut visible but disabled for non-admins", () => {
    const onManageFeatures = vi.fn();
    render(<ModulesFeatureGate isAdmin={false} onManageFeatures={onManageFeatures} />);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeVisible();
    expect(action).toBeDisabled();
    fireEvent.click(action);
    expect(onManageFeatures).not.toHaveBeenCalled();
  });
});
