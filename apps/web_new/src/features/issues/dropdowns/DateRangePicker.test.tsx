// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DateRangeCalendar, DateRangePicker } from "./DateRangePicker.js";

describe("DateRangeCalendar", () => {
  it("opens on the start month, else the end month, else the current month", () => {
    const first = render(<DateRangeCalendar value={{ start: "2026-10-10", end: "2026-11-20" }} onChange={() => {}} />);
    expect(screen.getByLabelText("Month")).toHaveValue("9");
    first.unmount();
    const second = render(<DateRangeCalendar value={{ start: null, end: "2026-11-20" }} onChange={() => {}} />);
    expect(screen.getByLabelText("Month")).toHaveValue("10");
    second.unmount();
    render(<DateRangeCalendar value={{ start: null, end: null }} onChange={() => {}} />);
    const now = new Date();
    expect(screen.getByLabelText("Month")).toHaveValue(String(now.getMonth()));
    expect(screen.getByLabelText("Year")).toHaveValue(String(now.getFullYear()));
  });

  it("keeps the viewed month when the value changes", () => {
    const { rerender } = render(<DateRangeCalendar value={{ start: "2026-10-10", end: null }} onChange={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(screen.getByLabelText("Month")).toHaveValue("10");
    rerender(<DateRangeCalendar value={{ start: "2026-10-10", end: "2026-11-20" }} onChange={() => {}} />);
    expect(screen.getByLabelText("Month")).toHaveValue("10");
  });

  it("clamps out-of-range anchor years into the caption range", () => {
    render(<DateRangeCalendar value={{ start: "2200-05-05", end: null }} onChange={() => {}} />);
    expect(screen.getByLabelText("Year")).toHaveValue("2100");
  });

  it("navigates with buttons and selects", () => {
    render(<DateRangeCalendar value={{ start: "2026-10-10", end: "2026-10-20" }} onChange={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Previous month" }));
    expect(screen.getByLabelText("Month")).toHaveValue("8");
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    fireEvent.change(screen.getByLabelText("Month"), { target: { value: "0" } });
    fireEvent.change(screen.getByLabelText("Year"), { target: { value: "2025" } });
    expect(screen.getByRole("button", { name: "January 15, 2025" })).toBeInTheDocument();
  });

  it("rotates weekday headers onto the week start", () => {
    render(<DateRangeCalendar value={{ start: "2026-10-10", end: "2026-10-20" }} onChange={() => {}} weekStart={1} />);
    expect(screen.getAllByTestId(/date-range-calendar-weekday-/).map((header) => header.textContent)).toEqual([
      "Mo",
      "Tu",
      "We",
      "Th",
      "Fr",
      "Sa",
      "Su",
    ]);
  });

  it("moves one end per click and resets on a same-day double pick", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <DateRangeCalendar value={{ start: "2026-10-10", end: "2026-10-20" }} onChange={onChange} />
    );
    fireEvent.click(screen.getByRole("button", { name: "October 15, 2026" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ start: "2026-10-10", end: "2026-10-15" });
    rerender(<DateRangeCalendar value={{ start: "2026-10-10", end: null }} onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "October 10, 2026" }));
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenCalledWith({ start: null, end: null });
  });

  it("blocks disabled days", () => {
    const onChange = vi.fn();
    render(
      <DateRangeCalendar value={{ start: null, end: null }} onChange={onChange} min="2026-10-10" max="2026-10-20" />
    );
    const now = new Date();
    const grid = screen.getByTestId("date-range-calendar-grid");
    expect(grid.querySelectorAll("button")).toHaveLength(42);
    // The calendar opens on the current month with an empty value.
    const probe = now.getFullYear() === 2026 && now.getMonth() === 9 ? "October 9, 2026" : null;
    if (probe !== null) {
      expect(screen.getByRole("button", { name: probe })).toBeDisabled();
      fireEvent.click(screen.getByRole("button", { name: probe }));
      expect(onChange).not.toHaveBeenCalled();
    }
  });

  it("marks endpoints pressed and interior days in range", () => {
    render(<DateRangeCalendar value={{ start: "2026-10-10", end: "2026-10-20" }} onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "October 10, 2026" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "October 20, 2026" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "October 15, 2026" })).toHaveAttribute("data-in-range", "true");
    expect(screen.getByRole("button", { name: "October 15, 2026" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "October 25, 2026" })).not.toHaveAttribute("data-in-range");
  });

  it("reports raw day clicks when onDayClick replaces the mapping", () => {
    const onDayClick = vi.fn();
    render(<DateRangeCalendar value={{ start: "2026-10-10", end: "2026-10-20" }} onDayClick={onDayClick} />);
    fireEvent.click(screen.getByRole("button", { name: "October 15, 2026" }));
    expect(onDayClick).toHaveBeenCalledTimes(1);
    expect(onDayClick).toHaveBeenCalledWith("2026-10-15");
  });
});

describe("DateRangePicker", () => {
  it("labels the merged trigger with one smart label", () => {
    const { rerender } = render(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: "2026-10-10", end: "2026-10-20" }}
        onChange={() => {}}
      />
    );
    expect(screen.getByRole("button", { name: "Dates" })).toHaveTextContent("Oct 10 - 20, 2026");
    rerender(
      <DateRangePicker mode="merged" label="Dates" value={{ start: "2026-10-10", end: null }} onChange={() => {}} />
    );
    expect(screen.getByRole("button", { name: "Dates" })).toHaveTextContent("Oct 10, 2026 - End date");
    rerender(
      <DateRangePicker mode="merged" label="Dates" value={{ start: null, end: "2026-10-20" }} onChange={() => {}} />
    );
    expect(screen.getByRole("button", { name: "Dates" })).toHaveTextContent("Start date - Oct 20, 2026");
    rerender(<DateRangePicker mode="merged" label="Dates" value={{ start: null, end: null }} onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "Dates" })).toHaveTextContent("Select dates");
  });

  it("resolves day clicks through onDayClick alone, with no onChange mapping", () => {
    const onDayClick = vi.fn();
    render(
      <DateRangePicker mode="split" label="Dates" value={{ start: "2026-10-10", end: null }} onDayClick={onDayClick} />
    );
    fireEvent.click(screen.getByTestId("date-range-picker-trigger"));
    const calendar = screen.getByTestId("date-range-picker-calendar");
    fireEvent.click(within(calendar).getByRole("button", { name: "October 15, 2026" }));
    expect(onDayClick).toHaveBeenCalledTimes(1);
    expect(onDayClick).toHaveBeenCalledWith("2026-10-15");
  });

  it("hides the clear control while no onChange mapping is set", () => {
    render(
      <DateRangePicker
        mode="split"
        label="Dates"
        value={{ start: "2026-10-10", end: null }}
        onDayClick={() => {}}
        clearable
      />
    );
    expect(screen.queryByTestId("date-range-picker-clear")).not.toBeInTheDocument();
  });

  it("reports popup state through a controlled onOpenChange", () => {
    const onOpenChange = vi.fn();
    render(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: null, end: null }}
        onChange={() => {}}
        open={false}
        onOpenChange={onOpenChange}
      />
    );
    fireEvent.click(screen.getByTestId("date-range-picker-trigger"));
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });

  it("pins the default trigger at the caller's keyboard position", () => {
    const { rerender } = render(
      <DateRangePicker mode="split" label="Dates" value={{ start: null, end: null }} onChange={() => {}} tabIndex={3} />
    );
    expect(screen.getByTestId("date-range-picker-trigger")).toHaveAttribute("tabindex", "3");
    rerender(<DateRangePicker mode="split" label="Dates" value={{ start: null, end: null }} onChange={() => {}} />);
    // Unset leaves the trigger in natural order (the popover stamps 0).
    expect(screen.getByTestId("date-range-picker-trigger")).toHaveAttribute("tabindex", "0");
  });

  it("joins the split trigger spans with an arrow", () => {
    const { rerender } = render(
      <DateRangePicker mode="split" label="Dates" value={{ start: null, end: null }} onChange={() => {}} />
    );
    const trigger = screen.getByRole("button", { name: "Dates" });
    expect(trigger).toHaveTextContent("Start date");
    expect(trigger).toHaveTextContent("End date");
    rerender(
      <DateRangePicker
        mode="split"
        label="Dates"
        value={{ start: "2026-10-10", end: "2026-10-20" }}
        onChange={() => {}}
      />
    );
    expect(screen.getByRole("button", { name: "Dates" })).toHaveTextContent("Oct 10, 2026");
    expect(screen.getByRole("button", { name: "Dates" })).toHaveTextContent("Oct 20, 2026");
  });

  it("opens the calendar in a portal and stays open across picks", () => {
    const onChange = vi.fn();
    render(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: "2026-10-10", end: "2026-10-20" }}
        onChange={onChange}
      />
    );
    expect(screen.queryByTestId("date-range-picker-calendar")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("date-range-picker-trigger"));
    const calendar = screen.getByTestId("date-range-picker-calendar");
    expect(screen.getByTestId("date-range-picker")).not.toContainElement(calendar);
    fireEvent.click(within(calendar).getByRole("button", { name: "October 11, 2026" }));
    fireEvent.click(within(calendar).getByRole("button", { name: "October 5, 2026" }));
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenNthCalledWith(1, { start: "2026-10-10", end: "2026-10-11" });
    expect(onChange).toHaveBeenNthCalledWith(2, { start: "2026-10-05", end: "2026-10-20" });
    expect(screen.getByTestId("date-range-picker-calendar")).toBeInTheDocument();
  });

  it("clears both ends through one update when clearable", () => {
    const onChange = vi.fn();
    render(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: "2026-10-10", end: "2026-10-20" }}
        onChange={onChange}
        clearable
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear Dates" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ start: null, end: null });
  });

  it("hides the clear control by default, without ends, or when disabled", () => {
    const { rerender } = render(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: "2026-10-10", end: "2026-10-20" }}
        onChange={() => {}}
      />
    );
    expect(screen.queryByRole("button", { name: "Clear Dates" })).not.toBeInTheDocument();
    rerender(
      <DateRangePicker mode="merged" label="Dates" value={{ start: null, end: null }} onChange={() => {}} clearable />
    );
    expect(screen.queryByRole("button", { name: "Clear Dates" })).not.toBeInTheDocument();
    rerender(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: "2026-10-10", end: null }}
        onChange={() => {}}
        clearable
        disabled
      />
    );
    expect(screen.queryByRole("button", { name: "Clear Dates" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dates" })).toBeDisabled();
  });

  it("shows the clear control with a single end set", () => {
    const onChange = vi.fn();
    render(
      <DateRangePicker
        mode="split"
        label="Dates"
        value={{ start: null, end: "2026-10-20" }}
        onChange={onChange}
        clearable
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear Dates" }));
    expect(onChange).toHaveBeenCalledWith({ start: null, end: null });
  });

  it("does not open while disabled", () => {
    render(
      <DateRangePicker mode="split" label="Dates" value={{ start: null, end: null }} onChange={() => {}} disabled />
    );
    fireEvent.click(screen.getByTestId("date-range-picker-trigger"));
    expect(screen.queryByTestId("date-range-picker-calendar")).not.toBeInTheDocument();
  });

  it("threads min, max and week start into the calendar", () => {
    render(
      <DateRangePicker
        mode="merged"
        label="Dates"
        value={{ start: "2026-10-10", end: "2026-10-20" }}
        onChange={() => {}}
        min="2026-10-10"
        max="2026-10-20"
        weekStart={1}
      />
    );
    fireEvent.click(screen.getByTestId("date-range-picker-trigger"));
    const calendar = screen.getByTestId("date-range-picker-calendar");
    expect(within(calendar).getByRole("button", { name: "October 9, 2026" })).toBeDisabled();
    expect(
      within(calendar)
        .getAllByTestId(/date-range-picker-calendar-weekday-/)
        .map((header) => header.textContent)
    ).toEqual(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]);
  });
});
