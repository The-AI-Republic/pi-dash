// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline header (parity/specs/issues.md §6, ISS-044/045/046/047): the
// live count, the three zoom entries with their pressed state, Today and
// the fullscreen toggle. The header is presentational, so each test
// asserts the marker the parity driver reads plus the callback it fires.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { GanttHeader } from "./GanttHeader.js";
import { GANTT_ZOOMS } from "./scale.js";

function renderHeader(overrides: Partial<React.ComponentProps<typeof GanttHeader>> = {}) {
  const onZoom = vi.fn();
  const onToday = vi.fn();
  const onToggleFullscreen = vi.fn();
  const view = render(
    <GanttHeader
      rowCount={3}
      zoom="week"
      onZoom={onZoom}
      onToday={onToday}
      fullscreen={false}
      onToggleFullscreen={onToggleFullscreen}
      {...overrides}
    />
  );
  return { onZoom, onToday, onToggleFullscreen, ...view };
}

describe("GanttHeader", () => {
  it("counts rows with a singular form", () => {
    renderHeader({ rowCount: 1 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("1 work item");
  });

  it("counts rows with a plural form", () => {
    renderHeader({ rowCount: 7 });
    expect(screen.getByTestId("gantt-count")).toHaveTextContent("7 work items");
  });

  it("offers exactly the three zoom labels in order", () => {
    renderHeader();
    const group = screen.getByRole("group", { name: "Timeline zoom" });
    const buttons = group.querySelectorAll("button");
    expect([...buttons].map((button) => button.textContent)).toEqual(["Week", "Month", "Quarter"]);
    expect([...buttons].map((button) => button.dataset["zoomOption"])).toEqual([...GANTT_ZOOMS]);
  });

  it("marks the active zoom and switches to another on click", () => {
    const h = renderHeader({ zoom: "month" });
    const month = screen.getByRole("button", { name: "Month" });
    expect(month).toHaveAttribute("aria-pressed", "true");
    expect(month).toHaveAttribute("data-zoom-active", "true");
    expect(screen.getByRole("button", { name: "Week" })).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(screen.getByRole("button", { name: "Quarter" }));
    expect(h.onZoom).toHaveBeenCalledWith("quarter");
  });

  it("ignores a click on the already active zoom", () => {
    const h = renderHeader({ zoom: "week" });
    fireEvent.click(screen.getByRole("button", { name: "Week" }));
    expect(h.onZoom).not.toHaveBeenCalled();
  });

  it("re-centers on today", () => {
    const h = renderHeader();
    fireEvent.click(screen.getByTestId("gantt-today"));
    expect(h.onToday).toHaveBeenCalledTimes(1);
  });

  it("labels the expand control and toggles fullscreen", () => {
    const h = renderHeader();
    const control = screen.getByRole("button", { name: "Expand timeline" });
    expect(control).toHaveAttribute("data-testid", "gantt-fullscreen");
    fireEvent.click(control);
    expect(h.onToggleFullscreen).toHaveBeenCalledTimes(1);
  });

  it("labels the shrink control while fullscreen", () => {
    renderHeader({ fullscreen: true });
    expect(screen.getByRole("button", { name: "Exit full screen" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Expand timeline" })).not.toBeInTheDocument();
  });

  it("renders extra actions beside the controls", () => {
    renderHeader({ actions: <button type="button">Bulk</button> });
    expect(screen.getByRole("button", { name: "Bulk" })).toBeInTheDocument();
  });
});
