// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProgressRing } from "./ProgressRing";

describe("ProgressRing", () => {
  it("names the value and draws the arc to it", () => {
    const { container } = render(<ProgressRing percent={40} />);
    expect(screen.getByRole("img", { name: "40% complete" })).toBeInTheDocument();
    const arc = container.querySelectorAll("circle")[1]!;
    // 40% of a 13.5px-radius circumference (~84.82) is ~33.93.
    const dash = Number.parseFloat(arc.getAttribute("stroke-dasharray")!.split(" ")[0]!);
    expect(dash).toBeGreaterThan(33.9);
    expect(dash).toBeLessThan(34);
  });

  it("keeps the default middle number for whole percents", () => {
    render(<ProgressRing percent={7} />);
    expect(screen.getByText("7%")).toBeInTheDocument();
  });

  it("clamps a percent past the ends and refuses a non-finite one", () => {
    const { rerender } = render(<ProgressRing percent={140} />);
    expect(screen.getByRole("img", { name: "100% complete" })).toBeInTheDocument();
    rerender(<ProgressRing percent={-12} />);
    expect(screen.getByRole("img", { name: "0% complete" })).toBeInTheDocument();
    rerender(<ProgressRing percent={Number.NaN} />);
    expect(screen.getByRole("img", { name: "0% complete" })).toBeInTheDocument();
  });

  it("renders caller glyphs inside and takes a custom name", () => {
    render(
      <ProgressRing percent={100} label="Everything is done">
        <span data-testid="glyph">done</span>
      </ProgressRing>
    );
    expect(screen.getByRole("img", { name: "Everything is done" })).toBeInTheDocument();
    expect(screen.getByTestId("glyph")).toBeInTheDocument();
  });

  it("scales the small size down without losing the arc", () => {
    const { container } = render(<ProgressRing percent={50} size="small" />);
    const root = container.firstElementChild as HTMLElement;
    expect(root.style.width).toBe("24px");
    expect(container.querySelectorAll("circle")).toHaveLength(2);
  });
});
