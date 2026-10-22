// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectViewsGate } from "./ProjectViewsGate.js";

describe("ProjectViewsGate", () => {
  it("explains the disabled feature with an enabled shortcut for admins", () => {
    const onManageFeatures = vi.fn();
    render(<ProjectViewsGate isAdmin onManageFeatures={onManageFeatures} />);
    expect(screen.getByTestId("project-views-gate")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /Views are not enabled for the project\./ })).toBeInTheDocument();
    expect(screen.getByText("Views are not enabled for the project.")).toBeInTheDocument();
    const shortcut = screen.getByRole("button", { name: "Manage features" });
    expect(shortcut).toBeEnabled();
    fireEvent.click(shortcut);
    expect(onManageFeatures).toHaveBeenCalledTimes(1);
  });

  it("renders the shortcut disabled for non-admins", () => {
    render(<ProjectViewsGate isAdmin={false} onManageFeatures={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled();
  });
});
