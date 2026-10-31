// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom resume dialog (written from parity/specs/notifications.md §9,
// NTF-021). Two sections: a local-date calendar bounded at today (the
// shared issues DateCalendar, so both surfaces pick days the same way)
// and a half-hour time grid split into AM and PM. The resume moment needs
// both; submitting with either side empty keeps the dialog open. Slots
// that have already passed on today's date are not offered, and a day
// with nothing left says so instead of showing an empty grid.
import { Button, Dialog, Popover } from "@pidash/kit";
import { CalendarDays, ChevronDown } from "lucide-react";
import * as React from "react";

import { DateCalendar } from "../../issues/index.js";
import { composeResumeInstant, earliestSnoozeDay, offeredSlots, pickedDayLabel, slotLabels } from "../lib/snooze.js";

export type Meridiem = "AM" | "PM";

export const DATE_SECTION_HEADING = "Pick a date";
export const TIME_SECTION_HEADING = "Pick a time";
export const DATE_PLACEHOLDER = "Choose a day";
export const TIME_PLACEHOLDER = "Choose a time";
export const NO_SLOTS_LEFT = "No times left today — choose a later day.";
export const SNOOZE_BUSY_MESSAGE = "Hold on while the notification is snoozed.";
export const CUSTOM_SNOOZE_SUBMIT = "Submit";
export const CUSTOM_SNOOZE_CANCEL = "Cancel";

export interface CustomSnoozeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Submit the picked moment; resolves once the write settles. */
  onSubmit: (resume: Date) => Promise<unknown>;
  /** Clock source for the elapsed-slot rule; defaults to now. */
  now?: (() => number) | undefined;
}

interface PickState {
  day: string | null;
  slot: string | null;
  meridiem: Meridiem;
}

const EMPTY_PICK: PickState = { day: null, slot: null, meridiem: "AM" };

function Section({ heading, children }: { heading: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="flex flex-col gap-(--space-2)">
      <h3 className="text-emphasis font-medium text-(--text)">{heading}</h3>
      {children}
    </div>
  );
}

function DayTrigger({
  day,
  min,
  onPick,
}: {
  day: string | null;
  /** Earliest offered day, taken from the dialog's clock. */
  min: string;
  onPick: (iso: string) => void;
}): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger={
        <button
          type="button"
          aria-label={DATE_SECTION_HEADING}
          data-testid="snooze-date-picker"
          className="text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) text-(--text)"
        >
          <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
          {day === null ? DATE_PLACEHOLDER : pickedDayLabel(day)}
          <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      }
    >
      <DateCalendar
        value={day}
        min={min}
        max={null}
        testId="snooze-date-calendar"
        onSelect={(iso) => {
          onPick(iso);
          setOpen(false);
        }}
      />
    </Popover>
  );
}

function TimeTrigger({
  pick,
  onMeridiem,
  onSlot,
  slots,
  empty,
}: {
  pick: PickState;
  onMeridiem: (meridiem: Meridiem) => void;
  onSlot: (slot: string) => void;
  slots: string[];
  empty: boolean;
}): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      trigger={
        <button
          type="button"
          aria-label={TIME_SECTION_HEADING}
          data-testid="snooze-time-picker"
          className="text-body inline-flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) text-(--text)"
        >
          <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
          {pick.slot === null ? TIME_PLACEHOLDER : `${pick.slot} ${pick.meridiem}`}
          <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      }
    >
      <div role="listbox" aria-label={TIME_SECTION_HEADING} className="flex flex-col gap-(--space-2)">
        <div className="flex gap-(--space-1)">
          {(["AM", "PM"] as const).map((meridiem) => (
            <button
              key={meridiem}
              type="button"
              aria-pressed={pick.meridiem === meridiem}
              onClick={() => onMeridiem(meridiem)}
              className={
                pick.meridiem === meridiem
                  ? "text-body rounded-(--radius-control) bg-(--accent-subtle) px-(--space-2) py-(--space-1) font-medium text-(--text)"
                  : "text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle)"
              }
            >
              {meridiem}
            </button>
          ))}
        </div>
        {empty ? (
          <p data-testid="snooze-no-slots" className="text-body text-(--text-muted)">
            {NO_SLOTS_LEFT}
          </p>
        ) : (
          <div className="grid max-h-56 grid-cols-3 gap-(--space-1) overflow-y-auto">
            {slots.map((slot) => (
              <button
                key={slot}
                type="button"
                role="option"
                aria-selected={pick.slot === slot}
                onClick={() => {
                  onSlot(slot);
                  setOpen(false);
                }}
                className={
                  pick.slot === slot
                    ? "text-body rounded-(--radius-control) bg-(--accent-subtle) px-(--space-2) py-(--space-1) font-medium text-(--text)"
                    : "text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text) hover:bg-(--subtle)"
                }
              >
                {slot}
              </button>
            ))}
          </div>
        )}
      </div>
    </Popover>
  );
}

/**
 * Controlled dialog. The pick resets each time it opens; submit is enabled
 * only with a day and a slot, and keeps the dialog open (with a hint) when
 * either is missing.
 */
export function CustomSnoozeDialog({
  open,
  onOpenChange,
  onSubmit,
  now = Date.now,
}: CustomSnoozeDialogProps): React.ReactElement {
  const [pick, setPick] = React.useState<PickState>(EMPTY_PICK);
  const [submitting, setSubmitting] = React.useState(false);
  const [missing, setMissing] = React.useState(false);
  const nowMs = now();

  React.useEffect(() => {
    if (open) {
      setPick(EMPTY_PICK);
      setMissing(false);
      setSubmitting(false);
    }
  }, [open]);

  const slots = React.useMemo(() => {
    if (pick.day === null) return slotLabels();
    return offeredSlots(pick.day, nowMs, pick.meridiem);
  }, [pick.day, pick.meridiem, nowMs]);
  const empty = pick.day !== null && slots.length === 0;

  React.useEffect(() => {
    if (pick.slot !== null && !slots.includes(pick.slot)) {
      setPick((current) => ({ ...current, slot: null }));
    }
  }, [pick.slot, slots]);

  React.useEffect(() => {
    if (empty && pick.meridiem === "AM") {
      setPick((current) => ({ ...current, meridiem: "PM" }));
    }
  }, [empty, pick.meridiem]);

  const submit = (): void => {
    const resume = composeResumeInstant(pick.day, pick.slot, pick.meridiem);
    if (resume === null) {
      setMissing(true);
      return;
    }
    setMissing(false);
    setSubmitting(true);
    void onSubmit(resume).then(
      () => onOpenChange(false),
      () => setSubmitting(false)
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      nonDismissable={submitting}
      title="Snooze until"
      description="Pick the day and the time the notification returns."
      className="max-w-lg"
      actions={
        <>
          <Button variant="ghost" disabled={submitting} onClick={() => onOpenChange(false)}>
            {CUSTOM_SNOOZE_CANCEL}
          </Button>
          <Button variant="primary" loading={submitting} disabled={submitting} onClick={submit}>
            {CUSTOM_SNOOZE_SUBMIT}
          </Button>
        </>
      }
    >
      <div data-testid="snooze-custom-dialog" className="flex flex-col gap-(--space-8)">
        <Section heading={DATE_SECTION_HEADING}>
          <DayTrigger
            day={pick.day}
            min={earliestSnoozeDay(nowMs)}
            onPick={(iso) => setPick((current) => ({ ...current, day: iso, slot: null }))}
          />
        </Section>
        <Section heading={TIME_SECTION_HEADING}>
          <TimeTrigger
            pick={pick}
            onMeridiem={(meridiem) => setPick((current) => ({ ...current, meridiem }))}
            onSlot={(slot) => setPick((current) => ({ ...current, slot }))}
            slots={slots}
            empty={empty}
          />
        </Section>
        {missing ? (
          <p role="alert" data-testid="snooze-missing-parts" className="text-body text-(--danger)">
            Choose a day and a time to continue.
          </p>
        ) : null}
        {submitting ? (
          <p role="status" data-testid="snooze-busy" className="text-body text-(--text-muted)">
            {SNOOZE_BUSY_MESSAGE}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}
