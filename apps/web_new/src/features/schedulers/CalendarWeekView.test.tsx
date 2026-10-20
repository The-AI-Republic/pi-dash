// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Week grid (parity/specs/agents.md §5, AGT-015/062): seven hour lanes at
// 48px, each firing a titled button at its minute offset, the today pill
// and the live time line on today only, and no drag or export affordance.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerOccurrence } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CalendarWeekView } from "./CalendarWeekView.js";

function occurrence(dtstart: string, overrides: Partial<SchedulerOccurrence> = {}): SchedulerOccurrence {
  return {
    binding_id: "b1",
    scheduler_id: "s1",
    scheduler_name: "Weekly",
    scheduler_color: "rgb(94, 106, 210)",
    dtstart,
    tzid: "UTC",
    kind: "scheduled",
    agent_run_id: null,
    status: null,
    ...overrides,
  };
}

const ANCHOR = new Date(2026, 9, 21, 10, 0); // Wednesday 21 October 2026
const TODAY = new Date(2026, 9, 21, 12, 15);

describe("CalendarWeekView", () => {
  it("renders a labelled header and seven 24-hour lanes", () => {
    const { container } = render(<CalendarWeekView anchor={ANCHOR} now={TODAY} byDay={new Map()} onSelect={vi.fn()} />);
    const header = screen.getByText("Sun 18").parentElement as HTMLElement;
    expect(within(header.parentElement as HTMLElement).getAllByText(/^\w{3} \d{1,2}$/)).toHaveLength(7);
    expect(container.querySelectorAll('div[style*="1152px"]')).toHaveLength(7);
  });

  it("marks today, draws the live line and clears both off today", () => {
    const { container, rerender } = render(
      <CalendarWeekView anchor={ANCHOR} now={TODAY} byDay={new Map()} onSelect={vi.fn()} />
    );
    expect(screen.getByText("Today")).toBeInTheDocument();
    expect((container.querySelector("div.border-t-2") as HTMLElement).style.top).toBe("588px");
    const nextWeek = new Date(2026, 9, 28, 9, 0);
    rerender(<CalendarWeekView anchor={nextWeek} now={TODAY} byDay={new Map()} onSelect={vi.fn()} />);
    expect(screen.queryByText("Today")).not.toBeInTheDocument();
    expect(container.querySelector("div.border-t-2")).toBeNull();
  });

  it("places blocks at their minute offset with their name and time", async () => {
    const onSelect = vi.fn();
    const firing = occurrence(new Date(2026, 9, 21, 9, 30).toISOString());
    const byDay = new Map([["2026-10-21", [firing]]]);
    render(<CalendarWeekView anchor={ANCHOR} now={TODAY} byDay={byDay} onSelect={onSelect} />);
    const block = screen.getByTitle(/Weekly/);
    expect(block.tagName).toBe("BUTTON");
    expect((block as HTMLElement).style.top).toBe("456px");
    expect(block).not.toHaveAttribute("draggable");
    expect(within(block).getByText("Weekly")).toBeInTheDocument();
    await userEvent.click(block);
    expect(onSelect).toHaveBeenCalledWith(firing);
  });

  it("shades a past firing differently from a future one", () => {
    const byDay = new Map([
      ["2026-10-20", [occurrence(new Date(2026, 9, 20, 8, 0).toISOString(), { kind: "past", agent_run_id: "r1" })]],
      ["2026-10-22", [occurrence(new Date(2026, 9, 22, 8, 0).toISOString())]],
    ]);
    render(<CalendarWeekView anchor={ANCHOR} now={TODAY} byDay={byDay} onSelect={vi.fn()} />);
    const [past, future] = screen.getAllByTitle(/Weekly/);
    expect((past as HTMLElement).style.backgroundColor).toBe("rgb(229, 231, 235)");
    expect((future as HTMLElement).style.backgroundColor).toBe("rgb(94, 106, 210)");
  });
});
