// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The sidebar toggle labels and announces its current side, and calls
// back on activation (MOD-032).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ModuleSidebarToggle } from "./ModuleSidebarToggle.js";

describe("ModuleSidebarToggle", () => {
  it("collapses the visible sidebar", () => {
    const onToggle = vi.fn();
    render(<ModuleSidebarToggle collapsed={false} onToggle={onToggle} />);
    const toggle = screen.getByTestId("module-detail-sidebar-toggle");
    expect(toggle).toHaveAccessibleName("Collapse sidebar");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("expands the hidden sidebar", () => {
    const onToggle = vi.fn();
    render(<ModuleSidebarToggle collapsed onToggle={onToggle} />);
    const toggle = screen.getByTestId("module-detail-sidebar-toggle");
    expect(toggle).toHaveAccessibleName("Expand sidebar");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});
