// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The group heading is a disclosure button with the count inside;
// collapsing hides the rows and keeps the count.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CycleGroup } from "./CycleGroup.js";

describe("CycleGroup", () => {
  it("renders the heading with its count and expanded rows", () => {
    const onToggle = vi.fn();
    render(
      <CycleGroup group="upcoming" count={2} expanded onToggle={onToggle}>
        <span>row</span>
      </CycleGroup>
    );
    const heading = screen.getByRole("button", { name: "Upcoming cycle 2" });
    expect(heading).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("row")).toBeInTheDocument();
    fireEvent.click(heading);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("hides rows while collapsed and keeps the count visible", () => {
    render(
      <CycleGroup group="completed" count={1} expanded={false} onToggle={() => undefined}>
        <span>row</span>
      </CycleGroup>
    );
    const heading = screen.getByRole("button", { name: "Completed cycle 1" });
    expect(heading).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("row")).not.toBeInTheDocument();
    expect(screen.getByTestId("cycle-group-count")).toHaveTextContent("1");
  });

  it("renders empty groups with a zero count", () => {
    render(<CycleGroup group="upcoming" count={0} expanded onToggle={() => undefined} />);
    expect(screen.getByRole("button", { name: "Upcoming cycle 0" })).toBeInTheDocument();
  });

  it("lists rows plain by default with no gallery wrapper", () => {
    render(
      <CycleGroup group="upcoming" count={1} expanded onToggle={() => undefined}>
        <span>row</span>
      </CycleGroup>
    );
    expect(screen.queryByTestId("cycle-group-upcoming-gallery")).not.toBeInTheDocument();
    expect(screen.getByText("row")).toBeInTheDocument();
  });

  it("wraps the same rows in the gallery grid without touching the heading", () => {
    const onToggle = vi.fn();
    render(
      <CycleGroup group="completed" count={1} expanded onToggle={onToggle} layout="gallery">
        <span>row</span>
      </CycleGroup>
    );
    const gallery = screen.getByTestId("cycle-group-completed-gallery");
    expect(gallery).toHaveTextContent("row");
    expect(screen.getByRole("button", { name: "Completed cycle 1" })).toHaveAttribute("aria-expanded", "true");
  });

  it("hides the gallery while collapsed", () => {
    render(
      <CycleGroup group="completed" count={1} expanded={false} onToggle={() => undefined} layout="gallery">
        <span>row</span>
      </CycleGroup>
    );
    expect(screen.queryByTestId("cycle-group-completed-gallery")).not.toBeInTheDocument();
  });
});
