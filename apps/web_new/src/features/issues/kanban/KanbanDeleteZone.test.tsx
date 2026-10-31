// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Board delete zone (parity/specs/issues.md §5, ISS-040): mounted but
// hidden while idle, faded in during a drag, red under the pointer.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { KanbanDeleteZone } from "./KanbanDeleteZone.js";

describe("KanbanDeleteZone", () => {
  it("stays mounted but hidden while idle", () => {
    render(<KanbanDeleteZone active={false} hovered={false} />);
    const zone = screen.getByTestId("kanban-delete-zone");
    expect(zone).toHaveAttribute("aria-hidden", "true");
    expect(zone.getAttribute("data-active")).toBeNull();
    expect(zone.className).toMatch(/opacity-0/);
  });

  it("fades in during a drag", () => {
    render(<KanbanDeleteZone active={true} hovered={false} />);
    const zone = screen.getByTestId("kanban-delete-zone");
    expect(zone).toHaveAttribute("data-active", "true");
    expect(zone).toHaveAttribute("aria-hidden", "false");
    expect(zone.className).toMatch(/opacity-100/);
    expect(screen.getByText("Release to delete this work item")).toBeVisible();
  });

  it("turns red under the pointer", () => {
    render(<KanbanDeleteZone active={true} hovered={true} />);
    const zone = screen.getByTestId("kanban-delete-zone");
    expect(zone).toHaveAttribute("data-hovered", "true");
    expect(screen.getByText("Release to delete this work item").className).toMatch(/text-\(--danger\)/);
  });
});
