// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Month grid (parity/specs/agents.md §5, AGT-015/016/062): whole weeks
// with out-of-month cells dimmed, today's numeral filled, up to four
// blocks per day plus "+ N more", the density rollup on a truncated
// window, neighbour-month days left blank, and no drag or export.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SchedulerOccurrence } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { CalendarMonthView } from "./CalendarMonthView.js";

function occurrence(dtstart: string, overrides: Partial<SchedulerOccurrence> = {}): SchedulerOccurrence {
  return {
    binding_id: "b1",
    scheduler_id: "s1",
    scheduler_name: "Hourly",
    scheduler_color: "rgb(94, 106, 210)",
    dtstart,
    tzid: "UTC",
    kind: "scheduled",
    agent_run_id: null,
    status: null,
    ...overrides,
  };
}

const ANCHOR = new Date(2026, 9, 1);
const TODAY = new Date(2026, 9, 21, 12, 0);

function rendered(props: Partial<React.ComponentProps<typeof CalendarMonthView>> = {}) {
  const onSelect = vi.fn();
  const view = render(
    <CalendarMonthView anchor={ANCHOR} now={TODAY} byDay={new Map()} truncated={false} onSelect={onSelect} {...props} />
  );
  return { container: view.container, onSelect };
}

describe("CalendarMonthView", () => {
  it("renders whole weeks and a marked-less neighbour day", () => {
    const { container } = rendered();
    expect(container.querySelectorAll("div.min-h-\\[6rem\\]")).toHaveLength(35);
    expect(screen.getByText("Wed")).toBeInTheDocument();
    expect(screen.getByText("Sat")).toBeInTheDocument();
  });

  it("fills today's numeral and leaves other days plain", () => {
    const { container } = rendered();
    const pill = container.querySelector("span.rounded-full");
    expect(pill?.textContent).toBe("21");
    expect(pill).toHaveClass("bg-(--accent)");
  });

  it("marks no day when today is outside the grid", () => {
    const { container } = rendered({ now: new Date(2026, 11, 25) });
    expect(container.querySelector("span.rounded-full")).toBeNull();
  });

  it("draws blocks, folds the rest and selects on click", async () => {
    const firings = Array.from({ length: 24 }, (_, index) => occurrence(new Date(2026, 9, 21, 0, index).toISOString()));
    const { onSelect } = rendered({ byDay: new Map([["2026-10-21", firings]]) });
    const blocks = screen.getAllByTitle(/Hourly/);
    expect(blocks).toHaveLength(4);
    expect(blocks[0]).not.toHaveAttribute("draggable");
    expect(screen.getByRole("button", { name: "+ 20 more" })).toBeInTheDocument();
    await userEvent.click(blocks[0] as HTMLElement);
    expect(onSelect).toHaveBeenCalledWith(firings[0]);
  });

  it("opens the day's first firing from the overflow control", async () => {
    const firings = Array.from({ length: 6 }, (_, index) => occurrence(new Date(2026, 9, 21, 8, index).toISOString()));
    const { onSelect } = rendered({ byDay: new Map([["2026-10-21", firings]]) });
    await userEvent.click(screen.getByRole("button", { name: "+ 2 more" }));
    expect(onSelect).toHaveBeenCalledWith(firings[0]);
  });

  it("rolls a truncated day up per scheduler instead of stacking blocks", () => {
    const firings = Array.from({ length: 24 }, (_, index) => occurrence(new Date(2026, 9, 21, 0, index).toISOString()));
    rendered({ byDay: new Map([["2026-10-21", firings]]), truncated: true });
    expect(screen.getByRole("button", { name: "24× Hourly" })).toBeInTheDocument();
    expect(screen.queryAllByTitle(/Hourly/)).toHaveLength(0);
  });

  it("opens a scheduler's first firing of the day from a rollup line", async () => {
    const hourly = Array.from({ length: 3 }, (_, index) => occurrence(new Date(2026, 9, 21, 8, index).toISOString()));
    const daily = occurrence(new Date(2026, 9, 21, 7, 0).toISOString(), {
      binding_id: "b2",
      scheduler_id: "s2",
      scheduler_name: "Daily",
    });
    const { onSelect } = rendered({ byDay: new Map([["2026-10-21", [daily, ...hourly]]]), truncated: true });
    await userEvent.click(screen.getByRole("button", { name: "3× Hourly" }));
    expect(onSelect).toHaveBeenCalledWith(hourly[0]);
    await userEvent.click(screen.getByRole("button", { name: "1× Daily" }));
    expect(onSelect).toHaveBeenCalledWith(daily);
  });

  it("leaves a day without firings blank", () => {
    const { container } = rendered();
    const cells = [...container.querySelectorAll("div.min-h-\\[6rem\\]")];
    expect(within(cells[0] as HTMLElement).queryAllByRole("button")).toHaveLength(0);
  });

  it("shades past blocks", () => {
    const past = occurrence(new Date(2026, 9, 20, 8, 0).toISOString(), { kind: "past", agent_run_id: "r1" });
    rendered({ byDay: new Map([["2026-10-20", [past]]]) });
    expect((screen.getByTitle(/Hourly/) as HTMLElement).style.backgroundColor).toBe("rgb(229, 231, 235)");
  });
});
