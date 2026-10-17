// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MONTH_NAMES } from "./calendar.js";
import { DateCalendar, DatePicker } from "./DatePicker.js";

const MONTHS = MONTH_NAMES;

describe("DateCalendar", () => {
  it("opens on the value month with the caption selects set", () => {
    render(<DateCalendar value="2026-10-15" onSelect={() => {}} />);
    expect(screen.getByLabelText("Month")).toHaveValue("9");
    expect(screen.getByLabelText("Year")).toHaveValue("2026");
    expect(screen.getByRole("button", { name: "October 15, 2026" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "October 16, 2026" })).toHaveAttribute("aria-pressed", "false");
  });

  it("opens on the current month without a value", () => {
    render(<DateCalendar value={null} onSelect={() => {}} />);
    const now = new Date();
    expect(screen.getByLabelText("Month")).toHaveValue(String(now.getMonth()));
    expect(screen.getByLabelText("Year")).toHaveValue(String(now.getFullYear()));
  });

  it("opens on the current month for an invalid value", () => {
    render(<DateCalendar value="not-a-day" onSelect={() => {}} />);
    const now = new Date();
    expect(screen.getByLabelText("Month")).toHaveValue(String(now.getMonth()));
  });

  it("clamps out-of-range value years into the caption range", () => {
    const first = render(<DateCalendar value="2200-05-05" onSelect={() => {}} />);
    expect(screen.getByLabelText("Year")).toHaveValue("2100");
    expect(screen.getByLabelText("Month")).toHaveValue("4");
    first.unmount();
    render(<DateCalendar value="1800-01-10" onSelect={() => {}} />);
    expect(screen.getByLabelText("Year")).toHaveValue("1900");
  });

  it("steps months with the caption buttons", () => {
    render(<DateCalendar value="2026-10-15" onSelect={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Previous month" }));
    expect(screen.getByLabelText("Month")).toHaveValue("8");
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(screen.getByLabelText("Month")).toHaveValue("10");
  });

  it("jumps with the caption selects", () => {
    render(<DateCalendar value="2026-10-15" onSelect={() => {}} />);
    fireEvent.change(screen.getByLabelText("Month"), { target: { value: "0" } });
    fireEvent.change(screen.getByLabelText("Year"), { target: { value: "2025" } });
    expect(screen.getByRole("button", { name: "January 15, 2025" })).toBeInTheDocument();
  });

  it("rotates weekday headers onto the week start", () => {
    const { rerender } = render(<DateCalendar value="2026-10-15" onSelect={() => {}} weekStart={0} />);
    expect(screen.getByTestId("date-calendar-weekday-Su")).toHaveTextContent("Su");
    const headers = screen.getAllByTestId(/date-calendar-weekday-/);
    expect(headers.map((header) => header.textContent)).toEqual(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]);
    rerender(<DateCalendar value="2026-10-15" onSelect={() => {}} weekStart={1} />);
    expect(screen.getAllByTestId(/date-calendar-weekday-/).map((header) => header.textContent)).toEqual([
      "Mo",
      "Tu",
      "We",
      "Th",
      "Fr",
      "Sa",
      "Su",
    ]);
  });

  it("selects enabled days and blocks disabled ones", () => {
    const onSelect = vi.fn();
    render(<DateCalendar value="2026-10-15" onSelect={onSelect} min="2026-10-10" max="2026-10-20" />);
    const grid = screen.getByTestId("date-calendar-grid");
    // October 2026 opens on a Thursday with a Sunday start.
    expect(grid.querySelectorAll("button")).toHaveLength(42);
    expect(screen.getByRole("button", { name: "October 9, 2026" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "October 21, 2026" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "October 10, 2026" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "October 9, 2026" }));
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "October 15, 2026" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith("2026-10-15");
  });

  it("marks adjacent-month filler days", () => {
    render(<DateCalendar value="2026-10-15" onSelect={() => {}} />);
    expect(screen.getByRole("button", { name: "September 30, 2026" })).toHaveAttribute("data-in-month", "false");
    expect(screen.getByRole("button", { name: "October 1, 2026" })).toHaveAttribute("data-in-month", "true");
  });

  it("marks today in the current month", () => {
    render(<DateCalendar value={null} onSelect={() => {}} />);
    const now = new Date();
    const todayName = `${MONTHS[now.getMonth()]} ${now.getDate()}, ${now.getFullYear()}`;
    expect(screen.getByRole("button", { name: todayName })).toHaveAttribute("data-today", "true");
  });
});

describe("DatePicker", () => {
  it("shows the placeholder without a value and the label once set", () => {
    const { rerender } = render(<DatePicker label="Start date" value={null} onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "Start date" })).toHaveTextContent("Select date");
    rerender(<DatePicker label="Start date" value="2026-10-15" onChange={() => {}} />);
    expect(screen.getByRole("button", { name: "Start date" })).toHaveTextContent("Oct 15, 2026");
  });

  it("renders a custom placeholder", () => {
    render(<DatePicker label="Start date" value={null} onChange={() => {}} placeholder="Add start date" />);
    expect(screen.getByRole("button", { name: "Start date" })).toHaveTextContent("Add start date");
  });

  it("opens the calendar in a portal and closes on select", () => {
    const onChange = vi.fn();
    render(<DatePicker label="Start date" value="2026-10-01" onChange={onChange} />);
    expect(screen.queryByTestId("date-picker-calendar")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("date-picker-trigger"));
    const calendar = screen.getByTestId("date-picker-calendar");
    expect(screen.getByTestId("date-picker")).not.toContainElement(calendar);
    fireEvent.click(within(calendar).getByRole("button", { name: "October 15, 2026" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("2026-10-15");
    expect(screen.queryByTestId("date-picker-calendar")).not.toBeInTheDocument();
  });

  it("clears to null from the hover control", () => {
    const onChange = vi.fn();
    render(<DatePicker label="Due date" value="2026-10-15" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Clear Due date" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("hides the clear control without a value, when disabled, or when not clearable", () => {
    const { rerender } = render(<DatePicker label="Due date" value={null} onChange={() => {}} />);
    expect(screen.queryByRole("button", { name: "Clear Due date" })).not.toBeInTheDocument();
    rerender(<DatePicker label="Due date" value="2026-10-15" onChange={() => {}} disabled />);
    expect(screen.queryByRole("button", { name: "Clear Due date" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Due date" })).toBeDisabled();
    rerender(<DatePicker label="Due date" value="2026-10-15" onChange={() => {}} clearable={false} />);
    expect(screen.queryByRole("button", { name: "Clear Due date" })).not.toBeInTheDocument();
  });

  it("does not open while disabled", () => {
    render(<DatePicker label="Due date" value={null} onChange={() => {}} disabled />);
    fireEvent.click(screen.getByTestId("date-picker-trigger"));
    expect(screen.queryByTestId("date-picker-calendar")).not.toBeInTheDocument();
  });

  it("threads min, max and week start into the calendar", () => {
    render(
      <DatePicker
        label="Start date"
        value="2026-10-15"
        onChange={() => {}}
        min="2026-10-10"
        max="2026-10-20"
        weekStart={1}
      />
    );
    fireEvent.click(screen.getByTestId("date-picker-trigger"));
    const calendar = screen.getByTestId("date-picker-calendar");
    expect(within(calendar).getByRole("button", { name: "October 9, 2026" })).toBeDisabled();
    expect(
      within(calendar)
        .getAllByTestId(/date-picker-calendar-weekday-/)
        .map((header) => header.textContent)
    ).toEqual(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]);
  });
});
