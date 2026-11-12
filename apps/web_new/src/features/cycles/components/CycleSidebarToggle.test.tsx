// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collapse toggle: the label and the expanded state follow the panel,
// and the narrow mount carries its own test id.

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CYCLE_SIDEBAR_COLLAPSE_LABEL, CYCLE_SIDEBAR_EXPAND_LABEL, CycleSidebarToggle } from "./CycleSidebarToggle.js";

describe("CycleSidebarToggle", () => {
  it("offers collapse while the panel is visible", () => {
    const onToggle = vi.fn();
    render(<CycleSidebarToggle collapsed={false} onToggle={onToggle} />);
    const toggle = screen.getByTestId("cycle-sidebar-toggle");
    expect(toggle.getAttribute("aria-label")).toBe(CYCLE_SIDEBAR_COLLAPSE_LABEL);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("offers expand while the panel is hidden", () => {
    render(<CycleSidebarToggle collapsed onToggle={() => undefined} />);
    const toggle = screen.getByTestId("cycle-sidebar-toggle");
    expect(toggle.getAttribute("aria-label")).toBe(CYCLE_SIDEBAR_EXPAND_LABEL);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
  });

  it("renders under a custom test id for the narrow mount", () => {
    render(<CycleSidebarToggle collapsed={false} onToggle={() => undefined} testId="cycle-sidebar-toggle-narrow" />);
    expect(screen.getByTestId("cycle-sidebar-toggle-narrow")).not.toBeNull();
  });
});
