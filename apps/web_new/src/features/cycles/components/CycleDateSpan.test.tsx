// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The shared span renders the project-zone range with its own offset
// chip element and the viewer tooltip, and renders nothing dateless
// (CYC-044).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { CycleDateSpan } from "./CycleDateSpan.js";

const NY = "America/New_York";

describe("CycleDateSpan", () => {
  it("renders the range with the offset chip as its own element", () => {
    render(
      <CycleDateSpan
        startDate="2026-09-01T12:00:00Z"
        endDate="2026-12-31T12:00:00Z"
        projectTimeZone={NY}
        viewerTimeZone="UTC"
      />
    );
    expect(screen.getByTestId("cycle-date-span")).toHaveTextContent("Sep 01 - Dec 31, 2026");
    const chip = screen.getByTestId("cycle-date-offset");
    expect(chip).toHaveTextContent(/^UTC [+-]/);
    expect(chip).toHaveTextContent("UTC -04:00");
  });

  it("reveals the viewer equivalent on hover when zones differ", async () => {
    render(
      <CycleDateSpan
        startDate="2026-09-01T12:00:00Z"
        endDate="2026-12-31T12:00:00Z"
        projectTimeZone={NY}
        viewerTimeZone="UTC"
      />
    );
    const span = screen.getByTestId("cycle-date-span");
    fireEvent.focus(span);
    fireEvent.mouseEnter(span);
    const tip = await screen.findByRole("tooltip");
    expect(tip).toHaveTextContent(/your timezone/i);
    expect(tip).toHaveTextContent("2026");
  });

  it("shows no tooltip without a chip or in the same zone", () => {
    const { rerender, queryByRole } = render(
      <CycleDateSpan
        startDate="2026-09-01T12:00:00Z"
        endDate="2026-12-31T12:00:00Z"
        projectTimeZone="UTC"
        viewerTimeZone={NY}
      />
    );
    expect(screen.queryByTestId("cycle-date-offset")).not.toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByTestId("cycle-date-span"));
    expect(queryByRole("tooltip")).not.toBeInTheDocument();

    rerender(
      <CycleDateSpan
        startDate="2026-09-01T12:00:00Z"
        endDate="2026-12-31T12:00:00Z"
        projectTimeZone={NY}
        viewerTimeZone={NY}
      />
    );
    expect(screen.getByTestId("cycle-date-offset")).toBeInTheDocument();
    fireEvent.mouseEnter(screen.getByTestId("cycle-date-span"));
    expect(queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("renders nothing without dates", () => {
    const { container } = render(<CycleDateSpan startDate={null} endDate={null} projectTimeZone={NY} />);
    expect(container).toBeEmptyDOMElement();
  });
});
