// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PAGE-001 feature-off guidance and admin-only management control.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { PagesFeatureGate } from "./PagesFeatureGate.js";

describe("PagesFeatureGate", () => {
  it("offers an enabled management shortcut to an admin", () => {
    const onManageFeatures = vi.fn();
    render(<PagesFeatureGate isAdmin onManageFeatures={onManageFeatures} />);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(onManageFeatures).toHaveBeenCalledTimes(1);
  });

  it("renders the shortcut disabled for a non-admin", () => {
    const onManageFeatures = vi.fn();
    render(<PagesFeatureGate isAdmin={false} onManageFeatures={onManageFeatures} />);
    const action = screen.getByRole("button", { name: "Manage features" });
    expect(action).toBeDisabled();
    fireEvent.click(action);
    expect(onManageFeatures).not.toHaveBeenCalled();
  });
});
