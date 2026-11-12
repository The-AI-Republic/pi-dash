// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics section guidance panel tests (parity/specs/analytics.md §6, ANL-006).
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AnalyticsSectionGuidance } from "./AnalyticsSectionGuidance.js";

describe("AnalyticsSectionGuidance", () => {
  it("renders the guidance copy inside the driver hook", () => {
    render(<AnalyticsSectionGuidance title="Nothing to plot yet" description="Add work to see it here." />);
    const panel = screen.getByTestId("analytics-section-guidance");
    expect(panel).toHaveTextContent("Nothing to plot yet");
    expect(panel).toHaveTextContent("Add work to see it here.");
  });
});
