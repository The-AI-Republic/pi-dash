// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The shared schedule editor (AGT-006/010/065): every field wires to the
// draft, the recurrence builder drives the rule, and the plain-language
// sentence stays live beneath it.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { emptySchedule } from "./schedule.js";
import { ScheduleFields, scheduleFieldIds, useScheduleDraft } from "./ScheduleFields.js";

const INITIAL = emptySchedule(new Date(2026, 9, 18, 12, 0));

function Harness({ onChange }: { onChange?: (next: string) => void }): React.ReactElement {
  const draft = useScheduleDraft(INITIAL);
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        onChange?.(`valid:${draft.validate()}`);
      }}
    >
      <ScheduleFields idPrefix="test" value={draft.value} errors={draft.errors} onChange={draft.change} />
      <button type="submit">Check</button>
    </form>
  );
}

describe("ScheduleFields", () => {
  it("wires ids, values and the plain-language sentence", () => {
    const change = vi.fn();
    render(<ScheduleFields idPrefix="test" value={INITIAL} errors={{}} onChange={change} />);
    const ids = scheduleFieldIds("test");
    expect(screen.getByLabelText("Starts at")).toHaveValue("2026-10-19T09:00");
    expect(screen.getByLabelText("Time zone")).toHaveValue(INITIAL.timeZone);
    expect(screen.getByLabelText("Recurrence (RRULE)")).toHaveValue("FREQ=DAILY");
    expect(screen.getByLabelText("Project context (optional)")).toHaveValue("");
    expect(screen.getByLabelText("Starts at")).toHaveAttribute("id", ids.startsAt);
    expect(screen.getByText("every day.")).toBeInTheDocument();
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("switch")).toHaveAccessibleName("Disable this install");
  });

  it("changes the rule through the builder and keeps the sentence live", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    expect(screen.getByText("every day.")).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Repeat unit"), "week");
    expect(screen.getByText("every week on Monday.")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Repeat interval"), { target: { value: "3" } });
    expect(screen.getByText("every 3 weeks on Monday.")).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("Repeat unit"), "none");
    expect(screen.getByText("Fires once at the start time.")).toBeInTheDocument();
  });

  it("reports an unreadable rule verbatim in the sentence", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.selectOptions(screen.getByLabelText("Repeat unit"), "none");
    await user.click(screen.getByRole("button", { name: "Advanced: rule text" }));
    await user.clear(screen.getByLabelText("Recurrence (RRULE)"));
    await user.type(screen.getByLabelText("Recurrence (RRULE)"), "FREQ=NOPE");
    expect(screen.getByText("FREQ=NOPE.")).toBeInTheDocument();
  });

  it("reports edits for every field and mirrors errors", async () => {
    const user = userEvent.setup();
    const change = vi.fn();
    render(
      <ScheduleFields
        idPrefix="test"
        value={{ ...INITIAL, enabled: false }}
        errors={{ startsAt: "Start time is required." }}
        onChange={change}
      />
    );
    await user.clear(screen.getByLabelText("Starts at"));
    expect(change).toHaveBeenCalledWith("startsAt", "");
    await user.selectOptions(screen.getByLabelText("Time zone"), "America/New_York");
    expect(change).toHaveBeenCalledWith("timeZone", "America/New_York");
    fireEvent.change(screen.getByLabelText("Project context (optional)"), { target: { value: "Notes" } });
    expect(change).toHaveBeenCalledWith("projectContext", "Notes");
    await user.click(screen.getByRole("switch"));
    expect(change).toHaveBeenCalledWith("enabled", true);
    expect(screen.getByRole("switch")).toHaveAccessibleName("Enable this install");
    expect(screen.getByText("Start time is required.")).toBeInTheDocument();
  });

  it("falls back to UTC for a blank zone and disables every control when busy", () => {
    const change = vi.fn();
    render(
      <ScheduleFields idPrefix="test" value={{ ...INITIAL, timeZone: "" }} errors={{}} disabled onChange={change} />
    );
    expect(screen.getByLabelText("Starts at")).toBeDisabled();
    expect(screen.getByLabelText("Time zone")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Advanced: rule text" })).toBeDisabled();
    expect(screen.getByLabelText("Project context (optional)")).toBeDisabled();
    expect(screen.getByRole("switch")).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "" } });
    expect(change).toHaveBeenCalledWith("timeZone", "UTC");
  });

  it("clears a field error when that field changes", async () => {
    const user = userEvent.setup();
    const check = vi.fn();
    render(<Harness onChange={check} />);
    await user.clear(screen.getByLabelText("Starts at"));
    await user.click(screen.getByRole("button", { name: "Check" }));
    expect(await screen.findByText("Start time is required.")).toBeInTheDocument();
    expect(check).toHaveBeenLastCalledWith("valid:false");
    await user.type(screen.getByLabelText("Starts at"), "2026-10-22T08:00");
    expect(screen.queryByText("Start time is required.")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Check" }));
    expect(check).toHaveBeenLastCalledWith("valid:true");
  });
});
