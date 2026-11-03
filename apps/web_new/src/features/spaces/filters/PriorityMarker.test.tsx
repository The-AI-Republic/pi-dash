// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PRIORITY_TONES } from "../../../shared/pickers/priorities.js";

import { PriorityMarker } from "./PriorityMarker.js";

describe("PriorityMarker", () => {
  it("marks every level with its shared glyph tone and stays hidden from assistive tech", () => {
    for (const level of ["urgent", "high", "medium", "low", "none"] as const) {
      const { unmount } = render(<PriorityMarker level={level} />);
      const marker = screen.getByTestId("space-priority-marker");
      expect(marker).toHaveAttribute("data-level", level);
      expect(marker).toHaveAttribute("aria-hidden", "true");
      expect(marker.querySelector("svg")).toHaveClass(PRIORITY_TONES[level]);
      unmount();
    }
  });

  it("sizes the glyph on request and defaults to the row size", () => {
    const { unmount } = render(<PriorityMarker level="high" />);
    expect(screen.getByTestId("space-priority-marker").querySelector("svg")).toHaveAttribute("width", "12");
    unmount();
    render(<PriorityMarker level="high" size={18} />);
    expect(screen.getByTestId("space-priority-marker").querySelector("svg")).toHaveAttribute("width", "18");
  });
});
