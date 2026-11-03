// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recurrence widget behavior (parity/specs/agents.md §14, AGT-065): the
// calendar-style controls serialize on interaction only, an inexpressible
// stored rule opens the raw editor untouched, and raw typing re-syncs the
// controls where the rule becomes expressible.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { RecurrenceBuilder, type RecurrenceBuilderProps } from "./RecurrenceBuilder.js";

const START = "2026-10-19T09:30";

function setup(value: string, overrides: Partial<RecurrenceBuilderProps> = {}) {
  const onChange = vi.fn();
  function Harness(): React.ReactElement {
    const [rule, setRule] = React.useState(value);
    return (
      <RecurrenceBuilder
        idPrefix="rb"
        value={rule}
        startsAt={START}
        {...overrides}
        onChange={(next) => {
          onChange(next);
          setRule(next);
        }}
      />
    );
  }
  render(<Harness />);
  return { onChange };
}

describe("RecurrenceBuilder", () => {
  it("serializes nothing merely by opening, and keeps raw collapsed for a simple rule", () => {
    const { onChange } = setup("FREQ=WEEKLY;BYDAY=MO,FR");
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Repeat interval")).toHaveValue(1);
    expect(screen.getByLabelText("Repeat unit")).toHaveValue("week");
    expect(screen.getByRole("button", { name: "Monday" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Friday" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Advanced: rule text" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByLabelText("Recurrence (RRULE)")).not.toBeVisible();
  });

  it("offers Sunday-first weekday chips in short labels", () => {
    setup("FREQ=WEEKLY");
    const chips = screen.getByRole("group", { name: "Repeat on" }).querySelectorAll("button");
    expect(Array.from(chips, (chip) => chip.textContent)).toEqual(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]);
  });

  it("changes the repeat interval and unit", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=DAILY");
    fireEvent.change(screen.getByLabelText("Repeat interval"), { target: { value: "3" } });
    expect(onChange).toHaveBeenLastCalledWith("FREQ=DAILY;INTERVAL=3");

    await user.selectOptions(screen.getByLabelText("Repeat unit"), "month");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;INTERVAL=3;BYMONTHDAY=19");
  });

  it("switches to the single-shot form and hides the repeat count", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=DAILY");
    await user.selectOptions(screen.getByLabelText("Repeat unit"), "none");
    expect(onChange).toHaveBeenLastCalledWith("");
    expect(screen.queryByLabelText("Repeat interval")).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Ends" })).not.toBeInTheDocument();
  });

  it("toggles weekday chips without emptying the set", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=WEEKLY;BYDAY=MO");
    await user.click(screen.getByRole("button", { name: "Thursday" }));
    expect(onChange).toHaveBeenLastCalledWith("FREQ=WEEKLY;BYDAY=MO,TH");
    expect(screen.getByRole("button", { name: "Thursday" })).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Monday" }));
    expect(onChange).toHaveBeenLastCalledWith("FREQ=WEEKLY;BYDAY=TH");
    expect(screen.getByRole("button", { name: "Thursday" })).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Thursday" }));
    expect(onChange).toHaveBeenLastCalledWith("FREQ=WEEKLY;BYDAY=TH");
  });

  it("edits the monthly day and the nth-weekday form", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=MONTHLY;BYMONTHDAY=19");
    const day = screen.getByLabelText("Day of the month");
    expect(day).toHaveValue(19);
    fireEvent.change(day, { target: { value: "7" } });
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYMONTHDAY=7");

    await user.selectOptions(screen.getByLabelText("Monthly pattern"), "weekday");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYDAY=1MO");

    await user.selectOptions(screen.getByLabelText("Weekday ordinal"), "3");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYDAY=3MO");
    await user.selectOptions(screen.getByLabelText("Weekday of the month"), "FR");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYDAY=3FR");
    await user.selectOptions(screen.getByLabelText("Weekday ordinal"), "-1");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYDAY=-1FR");
  });

  it("returns from the nth-weekday form to the monthly day form", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=MONTHLY;BYDAY=2WE");
    await user.selectOptions(screen.getByLabelText("Monthly pattern"), "day");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYMONTHDAY=8");
  });

  it("round-trips the monthly form through the day it lands on", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=MONTHLY;BYDAY=2WE");
    await user.selectOptions(screen.getByLabelText("Monthly pattern"), "day");
    expect(screen.getByLabelText("Day of the month")).toHaveValue(8);
    await user.selectOptions(screen.getByLabelText("Monthly pattern"), "weekday");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYDAY=2MO");
    expect(screen.getByLabelText("Weekday ordinal")).toHaveValue("2");
  });

  it("collapses the raw editor again after a disclosure toggle", async () => {
    const user = userEvent.setup();
    setup("FREQ=DAILY");
    const toggle = screen.getByRole("button", { name: "Advanced: rule text" });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByLabelText("Recurrence (RRULE)")).not.toBeVisible();
  });

  it("writes each end condition and keeps its value while switching", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=DAILY");
    await user.click(screen.getByRole("radio", { name: "On date" }));
    expect(onChange).toHaveBeenLastCalledWith("FREQ=DAILY;UNTIL=20271019T235959Z");
    expect(screen.getByLabelText("End date")).toHaveValue("2027-10-19");

    await user.click(screen.getByRole("radio", { name: "After" }));
    expect(onChange).toHaveBeenLastCalledWith("FREQ=DAILY;COUNT=10");

    fireEvent.change(screen.getByLabelText("Number of occurrences"), { target: { value: "4" } });
    expect(onChange).toHaveBeenLastCalledWith("FREQ=DAILY;COUNT=4");

    await user.click(screen.getByRole("radio", { name: "Never" }));
    expect(onChange).toHaveBeenLastCalledWith("FREQ=DAILY");
  });

  it("edits the on-date value", () => {
    const { onChange } = setup("FREQ=DAILY;UNTIL=20271231T235959Z");
    const date = screen.getByLabelText("End date");
    expect(date).toHaveValue("2027-12-31");
    fireEvent.change(date, { target: { value: "2028-02-01" } });
    expect(onChange).toHaveBeenLastCalledWith("FREQ=DAILY;UNTIL=20280201T235959Z");
  });

  it("opens an inexpressible rule in raw mode and never rewrites it on open", () => {
    const { onChange } = setup("FREQ=HOURLY");
    expect(onChange).not.toHaveBeenCalled();
    const raw = screen.getByLabelText("Recurrence (RRULE)");
    expect(raw).toBeVisible();
    expect(raw).toHaveValue("FREQ=HOURLY");
    expect(screen.getByText(/cannot show, so it is edited as text/)).toBeInTheDocument();
    // The rule cannot be hidden behind a disclosure, so there is no toggle.
    expect(screen.queryByRole("button", { name: "Advanced: rule text" })).not.toBeInTheDocument();
  });

  it("keeps an untouched stored rule byte-identical when it is only revealed", async () => {
    const user = userEvent.setup();
    const rule = "FREQ=WEEKLY;BYDAY=MO,WE";
    const { onChange } = setup(rule);
    await user.click(screen.getByRole("button", { name: "Advanced: rule text" }));
    expect(screen.getByLabelText("Recurrence (RRULE)")).toBeVisible();
    expect(screen.getByLabelText("Recurrence (RRULE)")).toHaveValue(rule);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("submits raw text as typed and re-syncs the controls when it is expressible", async () => {
    const user = userEvent.setup();
    const { onChange } = setup("FREQ=HOURLY");
    const raw = screen.getByLabelText("Recurrence (RRULE)");
    await user.clear(raw);
    await user.type(raw, "FREQ=MONTHLY;BYMONTHDAY=3");
    expect(onChange).toHaveBeenLastCalledWith("FREQ=MONTHLY;BYMONTHDAY=3");
    expect(screen.getByLabelText("Repeat unit")).toHaveValue("month");
    expect(screen.getByLabelText("Day of the month")).toHaveValue(3);
    // The raw field stays where the caret is even though the rule now
    // parses; the controls behind it show the same meaning.
    expect(raw).toBeVisible();
    expect(raw).toHaveValue("FREQ=MONTHLY;BYMONTHDAY=3");
    expect(screen.getByText(/Edit the RFC 5545 rule directly/)).toBeInTheDocument();
  });

  it("disables every control and refuses the advanced toggle while busy", () => {
    setup("FREQ=DAILY", { disabled: true });
    expect(screen.getByLabelText("Repeat interval")).toBeDisabled();
    expect(screen.getByLabelText("Repeat unit")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Advanced: rule text" })).toBeDisabled();
  });

  it("shows only the unit picker for the single-shot form", () => {
    setup("");
    expect(screen.getByLabelText("Repeat unit")).toHaveValue("none");
    expect(screen.getByLabelText("Repeat unit")).toHaveValue("none");
    expect(screen.queryByLabelText("Recurrence (RRULE)")).not.toBeVisible();
  });
});
