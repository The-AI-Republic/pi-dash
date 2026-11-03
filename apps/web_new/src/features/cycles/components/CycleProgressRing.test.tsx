// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The ring fills to the percent with the number inside, checks at
// exactly 100, and reads 0% on an empty arc.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { CycleProgressRing } from "./CycleProgressRing.js";

describe("CycleProgressRing", () => {
  it("fills to the percent with the number in the middle", () => {
    render(<CycleProgressRing percent={45} />);
    expect(screen.getByRole("img", { name: "45% complete" })).toBeInTheDocument();
    expect(screen.getByTestId("cycle-progress-ring")).toHaveTextContent("45%");
  });

  it("checks at exactly 100 instead of printing the number", () => {
    render(<CycleProgressRing percent={100} />);
    const ring = screen.getByRole("img", { name: "100% complete" });
    expect(ring).toBeInTheDocument();
    expect(ring.querySelector("svg.lucide-check")).not.toBeNull();
  });

  it("reads 0% on an empty ring", () => {
    render(<CycleProgressRing percent={0} />);
    expect(screen.getByRole("img", { name: "0% complete" })).toHaveTextContent("0%");
  });

  it("clamps out-of-range input into the arc", () => {
    const { rerender } = render(<CycleProgressRing percent={140} />);
    expect(screen.getByRole("img", { name: "100% complete" })).toBeInTheDocument();
    rerender(<CycleProgressRing percent={-4} />);
    expect(screen.getByRole("img", { name: "0% complete" })).toBeInTheDocument();
  });
});
