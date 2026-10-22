// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The gone-away view names the missing cycle and routes back to the
// list (CYC-013).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CycleGoneAway } from "./CycleGoneAway.js";

describe("CycleGoneAway", () => {
  it("routes back to the list on select", () => {
    const onBack = vi.fn();
    render(<CycleGoneAway onBack={onBack} />);
    expect(screen.getByTestId("cycles-gone-away")).toHaveTextContent("Cycle does not exist");
    fireEvent.click(screen.getByRole("button", { name: "View other cycles" }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});
