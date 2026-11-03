// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CalendarHeader } from "./CalendarHeader.js";

function stubViewport(narrow: boolean): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: narrow && query.includes("max-width"),
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
}

function renderHeader(overrides: Partial<React.ComponentProps<typeof CalendarHeader>> = {}) {
  const handlers = {
    onPrev: vi.fn(),
    onNext: vi.fn(),
    onToday: vi.fn(),
    onPickMonth: vi.fn(),
    onOptionsChange: vi.fn(),
  };
  render(
    <CalendarHeader
      title="October 2026"
      mode="month"
      showWeekends
      pickerYear={2026}
      pickerMonth0={9}
      {...handlers}
      {...overrides}
    />
  );
  return handlers;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("CalendarHeader", () => {
  it("steps, jumps to today, and opens the month picker", async () => {
    stubViewport(false);
    const handlers = renderHeader();
    const user = userEvent.setup();
    const stepBar = screen.getByRole("button", { name: "Previous" }).parentElement;
    const steps = stepBar?.querySelectorAll("button") ?? [];
    await user.click(steps[0] as HTMLElement);
    await user.click(steps[1] as HTMLElement);
    await user.click(screen.getByRole("button", { name: "Today" }));
    expect(handlers.onPrev).toHaveBeenCalledOnce();
    expect(handlers.onNext).toHaveBeenCalledOnce();
    expect(handlers.onToday).toHaveBeenCalledOnce();

    await user.click(screen.getByRole("button", { name: "October 2026" }));
    expect(screen.getByText("2026")).toBeInTheDocument();
    expect(screen.getAllByRole("button").filter((button) => button.textContent === "Jan")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Next year" }));
    expect(screen.getByText("2027")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Jan" }));
    expect(handlers.onPickMonth).toHaveBeenCalledWith(0, 2027);
  });

  it("disables the picker in week mode", () => {
    stubViewport(false);
    renderHeader({ mode: "week", title: "Oct 11 – Oct 17, 2026" });
    expect(screen.getByRole("button", { name: "Oct 11 – Oct 17, 2026" })).toBeDisabled();
  });

  it("switches layout and weekends from the desktop options menu", async () => {
    stubViewport(false);
    const handlers = renderHeader();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Options" }));
    await user.click(screen.getByRole("button", { name: /Week layout/ }));
    await user.click(screen.getByRole("button", { name: /Show weekends/ }));
    expect(handlers.onOptionsChange).toHaveBeenNthCalledWith(1, { mode: "week" });
    expect(handlers.onOptionsChange).toHaveBeenNthCalledWith(2, { showWeekends: false });
  });

  it("closes the compact options menu after a pick", async () => {
    stubViewport(true);
    const handlers = renderHeader();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Calendar options" }));
    await user.click(screen.getByRole("button", { name: /Month layout/ }));
    expect(handlers.onOptionsChange).toHaveBeenCalledWith({ mode: "month" });
    expect(screen.queryByRole("button", { name: /Month layout/ })).not.toBeInTheDocument();
  });

  it("tracks a changed picker year and closes on a month pick", async () => {
    stubViewport(false);
    const handlers = renderHeader();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "October 2026" }));
    await user.click(screen.getByRole("button", { name: "Previous year" }));
    expect(screen.getByText("2025")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Mar" }));
    expect(handlers.onPickMonth).toHaveBeenCalledWith(2, 2025);
    expect(screen.queryByText("2025")).not.toBeInTheDocument();
  });
});
