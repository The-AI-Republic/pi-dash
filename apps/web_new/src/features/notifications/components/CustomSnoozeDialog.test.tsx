// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom resume dialog (NTF-021): both a day and a time are required,
// future days carry the full half-hour grid, elapsed same-day slots are
// hidden, and twelve-hour picks compose the right instant (NEWFRONT-213
// target: 12 AM is midnight, 12 PM is noon).
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  CUSTOM_SNOOZE_CANCEL,
  CUSTOM_SNOOZE_SUBMIT,
  DATE_SECTION_HEADING,
  NO_SLOTS_LEFT,
  SNOOZE_BUSY_MESSAGE,
  TIME_SECTION_HEADING,
  CustomSnoozeDialog,
} from "./CustomSnoozeDialog.js";

const AFTERNOON = new Date(2026, 9, 17, 14, 10, 0, 0).getTime();

/** Local ISO day `offset` days from the pinned clock. */
function dayFrom(nowMs: number, offset: number): string {
  const date = new Date(nowMs);
  date.setDate(date.getDate() + offset);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

/** The calendar's assistive day name for a local ISO day. */
function dayName(day: string): RegExp {
  const [year, month, date] = day.split("-").map(Number) as [number, number, number];
  const monthName = new Date(year, month - 1, date).toLocaleString("en-US", { month: "long" });
  return new RegExp(`^${monthName} ${date}, ${year}$`);
}

function renderDialog(
  options: {
    open?: boolean;
    nowMs?: number;
    onSubmit?: (resume: Date) => Promise<unknown>;
  } = {}
) {
  const submitted: Date[] = [];
  const onSubmit = (resume: Date): Promise<unknown> => {
    submitted.push(resume);
    return options.onSubmit === undefined ? Promise.resolve() : options.onSubmit(resume);
  };
  const onOpenChange = vi.fn();
  render(
    <CustomSnoozeDialog
      open={options.open ?? true}
      onOpenChange={onOpenChange}
      onSubmit={onSubmit}
      now={() => options.nowMs ?? AFTERNOON}
    />
  );
  return { submitted, onSubmit, onOpenChange };
}

async function pickDay(user: ReturnType<typeof userEvent.setup>, day: string): Promise<void> {
  await user.click(screen.getByRole("button", { name: DATE_SECTION_HEADING }));
  await user.click(await screen.findByRole("button", { name: dayName(day) }));
}

async function pickTime(user: ReturnType<typeof userEvent.setup>, meridiem: "AM" | "PM", slot: string): Promise<void> {
  await user.click(screen.getByRole("button", { name: TIME_SECTION_HEADING }));
  const panel = await screen.findByRole("listbox", { name: TIME_SECTION_HEADING });
  await user.click(within(panel).getByRole("button", { name: new RegExp(`^${meridiem}$`) }));
  await user.click(within(panel).getByRole("option", { name: slot }));
}

describe("CustomSnoozeDialog", () => {
  it("resets the pick when reopened and keeps a picked time while it stays offered", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const onSubmit = (): Promise<unknown> => Promise.resolve();
    const clock = AFTERNOON;
    const view = render(
      <CustomSnoozeDialog open={false} onOpenChange={onOpenChange} onSubmit={onSubmit} now={() => clock} />
    );
    view.rerender(<CustomSnoozeDialog open onOpenChange={onOpenChange} onSubmit={onSubmit} now={() => clock} />);
    await screen.findByTestId("snooze-custom-dialog");
    expect(screen.getByRole("button", { name: DATE_SECTION_HEADING })).toHaveTextContent("Choose a day");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "AM", "10:30");
    expect(screen.getByRole("button", { name: TIME_SECTION_HEADING })).toHaveTextContent("10:30 AM");
    await user.click(screen.getByRole("button", { name: TIME_SECTION_HEADING }));
    const panel = await screen.findByRole("listbox", { name: TIME_SECTION_HEADING });
    await user.click(within(panel).getByRole("button", { name: /^PM$/ }));
    // The same slot is still offered in the afternoon, so it stays picked.
    expect(within(panel).getByRole("option", { name: "10:30" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: TIME_SECTION_HEADING })).toHaveTextContent("10:30 PM");
    // Two day/time picker round-trips in one case: give the full flow
    // room when the rest of the suite competes for the CPU.
  }, 30_000);

  it("renders nothing while closed", () => {
    renderDialog({ open: false });
    expect(screen.queryByTestId("snooze-custom-dialog")).not.toBeInTheDocument();
  });

  it("keeps the dialog open when submitting without a date or a time", async () => {
    const user = userEvent.setup();
    const { submitted, onOpenChange } = renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    expect(screen.getByTestId("snooze-custom-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("snooze-missing-parts")).toBeInTheDocument();
    expect(submitted).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("keeps the dialog open with a date but no time", async () => {
    const user = userEvent.setup();
    const { submitted, onOpenChange } = renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    expect(screen.getByTestId("snooze-custom-dialog")).toBeInTheDocument();
    expect(submitted).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("submits the composed instant once both parts are picked", async () => {
    const user = userEvent.setup();
    const { submitted, onOpenChange } = renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "AM", "10:30");
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0]?.getTime()).toBe(new Date(2026, 9, 19, 10, 30, 0, 0).getTime());
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("offers the full half-hour grid for a future day", async () => {
    const user = userEvent.setup();
    renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await user.click(screen.getByRole("button", { name: TIME_SECTION_HEADING }));
    const panel = await screen.findByRole("listbox", { name: TIME_SECTION_HEADING });
    expect(within(panel).getAllByRole("option")).toHaveLength(24);
    expect(within(panel).getByRole("option", { name: "10:30" })).toBeInTheDocument();
    await user.click(within(panel).getByRole("button", { name: /^PM$/ }));
    expect(within(panel).getAllByRole("option")).toHaveLength(24);
    expect(within(panel).getByRole("option", { name: "12:30" })).toBeInTheDocument();
  });

  it("hides the elapsed same-day slots", async () => {
    const user = userEvent.setup();
    renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 0));
    await user.click(screen.getByRole("button", { name: TIME_SECTION_HEADING }));
    const panel = await screen.findByRole("listbox", { name: TIME_SECTION_HEADING });
    // 14:10 has passed every morning slot and the current afternoon one.
    expect(within(panel).queryByRole("option", { name: "01:30" })).not.toBeInTheDocument();
    await user.click(within(panel).getByRole("button", { name: /^PM$/ }));
    expect(within(panel).queryByRole("option", { name: "01:00" })).not.toBeInTheDocument();
    expect(within(panel).queryByRole("option", { name: "02:00" })).not.toBeInTheDocument();
    expect(within(panel).getByRole("option", { name: "02:30" })).toBeInTheDocument();
  });

  it("says so when the picked day has no slot left", async () => {
    const user = userEvent.setup();
    const lateEvening = new Date(2026, 9, 17, 23, 45, 0, 0).getTime();
    renderDialog({ nowMs: lateEvening });
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(lateEvening, 0));
    await user.click(screen.getByRole("button", { name: TIME_SECTION_HEADING }));
    const panel = await screen.findByRole("listbox", { name: TIME_SECTION_HEADING });
    await user.click(within(panel).getByRole("button", { name: /^AM$/ }));
    await screen.findByTestId("snooze-no-slots");
    expect(screen.getByText(NO_SLOTS_LEFT)).toBeInTheDocument();
    expect(within(panel).queryAllByRole("option")).toHaveLength(0);
  });

  it("composes 12 PM to noon and 12 AM to midnight", async () => {
    const user = userEvent.setup();
    const { submitted } = renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "PM", "12:30");
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0]?.getTime()).toBe(new Date(2026, 9, 19, 12, 30, 0, 0).getTime());
  });

  it("composes a 12 AM pick to the start of the day", async () => {
    const user = userEvent.setup();
    const { submitted } = renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "AM", "12:30");
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0]?.getTime()).toBe(new Date(2026, 9, 19, 0, 30, 0, 0).getTime());
  });

  it("stays open when the write fails", async () => {
    const user = userEvent.setup();
    const failed = vi.fn(async () => {
      throw new Error("nope");
    });
    const { onOpenChange } = renderDialog({ onSubmit: failed });
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "AM", "10:30");
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    await waitFor(() => expect(failed).toHaveBeenCalledTimes(1));
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
    expect(screen.getByRole("button", { name: CUSTOM_SNOOZE_CANCEL })).not.toBeDisabled();
  });

  it("closes from the cancel control", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.click(await screen.findByRole("button", { name: CUSTOM_SNOOZE_CANCEL }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("clears a picked slot once the clock passes it", async () => {
    const user = userEvent.setup();
    let clock = new Date(2026, 9, 17, 8, 0, 0, 0).getTime();
    const onOpenChange = vi.fn();
    const view = render(
      <CustomSnoozeDialog open onOpenChange={onOpenChange} onSubmit={() => Promise.resolve()} now={() => clock} />
    );
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(clock, 0));
    await pickTime(user, "AM", "09:30");
    expect(screen.getByRole("button", { name: TIME_SECTION_HEADING })).toHaveTextContent("09:30 AM");
    clock = new Date(2026, 9, 17, 10, 0, 0, 0).getTime();
    view.rerender(
      <CustomSnoozeDialog open onOpenChange={onOpenChange} onSubmit={() => Promise.resolve()} now={() => clock} />
    );
    expect(screen.getByRole("button", { name: TIME_SECTION_HEADING })).toHaveTextContent("Choose a time");
  });

  it("stays open when a dismissal arrives while the write is in flight", async () => {
    const user = userEvent.setup();
    const pendingWrite = vi.fn(() => new Promise<unknown>(() => {}));
    renderDialog({ onSubmit: pendingWrite });
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "AM", "10:30");
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    await waitFor(() => expect(pendingWrite).toHaveBeenCalledTimes(1));
    await user.keyboard("{Escape}");
    // The write owns the dialog until it settles; the hint from the
    // dismissal attempt keeps it open and says what is happening.
    expect(screen.getByTestId("snooze-custom-dialog")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("snooze-busy")).toHaveTextContent(SNOOZE_BUSY_MESSAGE));
  });

  it("clears a time slot when the picked day no longer offers it", async () => {
    const user = userEvent.setup();
    const { submitted } = renderDialog();
    await screen.findByTestId("snooze-custom-dialog");
    await pickDay(user, dayFrom(AFTERNOON, 2));
    await pickTime(user, "AM", "10:30");
    // Switch to today, where 10:30 has passed: the slot clears and a
    // submit needs a new time.
    await pickDay(user, dayFrom(AFTERNOON, 0));
    await user.click(screen.getByRole("button", { name: CUSTOM_SNOOZE_SUBMIT }));
    expect(submitted).toHaveLength(0);
    expect(screen.getByTestId("snooze-missing-parts")).toBeInTheDocument();
  });
});
