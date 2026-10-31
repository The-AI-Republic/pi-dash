// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The no-match title stays fixed while the hint and the action name
// the cause: search offers its clear, filters offer the clear-all.

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CyclesNoMatch } from "./CyclesNoMatch.js";

describe("CyclesNoMatch", () => {
  it("offers the search clear on the search variant", () => {
    const onClearSearch = vi.fn();
    render(<CyclesNoMatch variant="search" onClearSearch={onClearSearch} />);
    expect(screen.getByRole("heading", { name: "No matching cycles" })).toBeInTheDocument();
    expect(screen.getByText("Remove the search criteria to see all cycles")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onClearSearch).toHaveBeenCalledTimes(1);
  });

  it("offers the filters clear-all on the filters variant", () => {
    const onClearFilters = vi.fn();
    render(<CyclesNoMatch variant="filters" onClearSearch={() => undefined} onClearFilters={onClearFilters} />);
    expect(screen.getByRole("heading", { name: "No matching cycles" })).toBeInTheDocument();
    expect(screen.getByText("Remove the filters to see all cycles")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear all filters" }));
    expect(onClearFilters).toHaveBeenCalledTimes(1);
  });

  it("tolerates a missing filters clear until slice C supplies it", () => {
    render(<CyclesNoMatch variant="filters" onClearSearch={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear all filters" }));
  });
});
