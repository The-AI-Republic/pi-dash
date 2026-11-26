// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One timeline row: the rail segment, the 28px type badge, and the rail
// trimmed at the first and last rows.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { TimelineRow, timelinePosition } from "./TimelineRow.js";

describe("timelinePosition", () => {
  it("trims both ends of a lone row", () => {
    expect(timelinePosition(0, 1)).toBe("only");
  });

  it("marks the first, middle and last rows", () => {
    expect(timelinePosition(0, 3)).toBe("first");
    expect(timelinePosition(1, 3)).toBe("middle");
    expect(timelinePosition(2, 3)).toBe("last");
  });
});

describe("TimelineRow", () => {
  it("paints the rail and hangs the badge on it", () => {
    const { container } = render(
      <TimelineRow position="middle" badgeLabel="Agent wait" badge={<span>hourglass</span>}>
        <p>row body</p>
      </TimelineRow>
    );
    expect(screen.getByRole("img", { name: "Agent wait" })).toBeInTheDocument();
    expect(screen.getByText("row body")).toBeInTheDocument();
    const rail = container.querySelector('[aria-hidden="true"]');
    expect(rail?.className).toContain("left-[13px]");
  });

  it("keeps the rail behind the badge", () => {
    const { container } = render(
      <TimelineRow position="middle" badgeLabel="Comment" badge={<span>bubble</span>}>
        <p>row body</p>
      </TimelineRow>
    );
    const badge = screen.getByRole("img", { name: "Comment" });
    expect(badge.className).toContain("z-[1]");
    expect(container.querySelector('[aria-hidden="true"]')?.className).toContain("absolute");
  });

  it("trims the rail at the first and last rows", () => {
    const first = render(
      <TimelineRow position="first" badgeLabel="State" badge={<span />}>
        <p>first</p>
      </TimelineRow>
    );
    const firstRow = first.container.firstElementChild as HTMLElement;
    expect(firstRow.className).toContain("pb-(--space-2)");
    expect(firstRow.className).not.toContain("pt-(--space-2)");
    first.unmount();

    const last = render(
      <TimelineRow position="last" badgeLabel="State" badge={<span />}>
        <p>last</p>
      </TimelineRow>
    );
    const lastRow = last.container.firstElementChild as HTMLElement;
    expect(lastRow.className).toContain("pt-(--space-2)");
    expect(lastRow.className).not.toContain("pb-(--space-2)");
    last.unmount();

    const only = render(
      <TimelineRow position="only" badgeLabel="State" badge={<span />}>
        <p>only</p>
      </TimelineRow>
    );
    const onlyRow = only.container.firstElementChild as HTMLElement;
    expect(onlyRow.className).not.toContain("pb-(--space-2)");
    expect(onlyRow.className).not.toContain("pt-(--space-2)");
    only.unmount();

    const middle = render(
      <TimelineRow position="middle" badgeLabel="State" badge={<span />}>
        <p>middle</p>
      </TimelineRow>
    );
    const middleRow = middle.container.firstElementChild as HTMLElement;
    expect(middleRow.className).toContain("py-(--space-2)");
  });
});
