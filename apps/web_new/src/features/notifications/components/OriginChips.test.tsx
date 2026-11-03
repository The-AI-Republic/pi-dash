// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { OriginChips } from "./OriginChips.js";

describe("OriginChips (NTF-015)", () => {
  it("renders nothing while no origin is active", () => {
    const { container } = render(<OriginChips origins={[]} onRemove={vi.fn()} onClearAll={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("group", { name: "Applied filters" })).not.toBeInTheDocument();
  });

  it("shows one removable chip per active origin, in filter order", () => {
    render(<OriginChips origins={["assigned", "subscribed"]} onRemove={vi.fn()} onClearAll={vi.fn()} />);
    const chips = screen.getAllByTestId("inbox-origin-chip");
    expect(chips.map((chip) => chip.getAttribute("data-origin"))).toEqual(["assigned", "subscribed"]);
    for (const chip of chips) {
      expect(within(chip).getByRole("button", { name: /^Remove filter / })).toBeInTheDocument();
    }
    expect(chips[0]?.textContent).toContain("Assigned to me");
  });

  it("removes the chip's origin when its delete control is pressed", () => {
    const onRemove = vi.fn();
    render(<OriginChips origins={["assigned", "created"]} onRemove={onRemove} onClearAll={vi.fn()} />);
    fireEvent.click(screen.getByTestId("inbox-origin-chip-remove-created"));
    expect(onRemove).toHaveBeenCalledWith("created");
  });

  it("clears every origin from the clear-all chip", () => {
    const onClearAll = vi.fn();
    render(<OriginChips origins={["created"]} onRemove={vi.fn()} onClearAll={onClearAll} />);
    fireEvent.click(screen.getByTestId("inbox-origin-clear-all"));
    expect(onClearAll).toHaveBeenCalledTimes(1);
  });
});
