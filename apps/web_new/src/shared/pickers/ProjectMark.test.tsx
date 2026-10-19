// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ProjectMark } from "./ProjectMark.js";

describe("ProjectMark", () => {
  it("draws an emoji mark as its glyph", () => {
    render(<ProjectMark logoProps={{ in_use: "emoji", emoji: { value: "🌱" } }} identifier="SEED" />);
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "emoji");
    expect(mark).toHaveTextContent("🌱");
    expect(mark.querySelector("svg")).toBeNull();
  });

  it("draws a configured icon in its own color at the requested size", () => {
    render(
      <ProjectMark
        logoProps={{ in_use: "icon", icon: { name: "rocket_launch", color: "#f97316" } }}
        identifier="SEED"
        size={24}
        className="custom"
      />
    );
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "icon");
    expect(mark).toHaveStyle({ width: "24px", height: "24px" });
    expect(mark).toHaveClass("custom");
    const glyph = mark.querySelector("svg");
    expect(glyph).not.toBeNull();
    expect(glyph!.getAttribute("stroke")).toBe("#f97316");
    expect(glyph!.getAttribute("width")).toBe("17");
  });

  it("leaves the icon in its inherited color when none is stored", () => {
    render(<ProjectMark logoProps={{ in_use: "icon", icon: { name: "star" } }} identifier="SEED" size={16} />);
    const glyph = screen.getByTestId("project-mark").querySelector("svg");
    expect(glyph).not.toBeNull();
    expect(glyph!.getAttribute("stroke")).toBe("currentColor");
  });

  it("falls back to the initial for an unknown icon or an empty logo", () => {
    const { rerender } = render(
      <ProjectMark logoProps={{ in_use: "icon", icon: { name: "no_such_symbol" } }} identifier="SEED" />
    );
    expect(screen.getByTestId("project-mark")).toHaveAttribute("data-mark", "initial");
    expect(screen.getByTestId("project-mark")).toHaveTextContent("S");
    rerender(<ProjectMark logoProps={{}} identifier="" />);
    expect(screen.getByTestId("project-mark")).toHaveTextContent("?");
  });

  it("stays decorative for assistive tech", () => {
    render(<ProjectMark logoProps={{}} identifier="SEED" />);
    expect(screen.getByTestId("project-mark")).toHaveAttribute("aria-hidden", "true");
  });
});
