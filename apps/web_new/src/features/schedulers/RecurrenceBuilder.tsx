// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recurrence widget for the schedule editor (parity/specs/agents.md §14,
// AGT-065). Calendar-style controls over the stored rule string: a repeat
// count and unit, weekday chips, a monthly day-or-weekday choice and an
// end condition. The raw rule stays available under an advanced
// disclosure; a stored rule the widget cannot express starts there and is
// never rewritten unless the user edits it.
//
// The widget is a pure function of the rule it is handed: every
// interaction serializes a complete next rule, and a rule arriving from
// elsewhere (raw typing, a different save) simply re-parses. Nothing is
// serialized merely by rendering.

import * as React from "react";

import { cn } from "@pidash/kit";
import {
  DEFAULT_AFTER_COUNT,
  REPEAT_UNITS,
  UNIT_LABELS,
  WEEKDAY_CODES,
  WEEKDAY_LABELS,
  defaultEndDate,
  parseRecurrence,
  serializeRecurrence,
  withMonthlyMode,
  withUnit,
  withWeekdayToggled,
  type EndsChoice,
  type MonthlyChoice,
  type RepeatUnit,
  type RecurrenceState,
  type WeekdayCode,
} from "./recurrence.js";

export interface RecurrenceBuilderProps {
  idPrefix: string;
  /** The stored bare rule (empty means one firing at the start). */
  value: string;
  /** datetime-local start the monthly and on-date defaults follow. */
  startsAt: string;
  disabled?: boolean;
  onChange: (rule: string) => void;
}

const FOCUS_RING =
  "focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none";

const controlClass =
  "text-body h-(--control-height) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50";

function changeEnds(state: RecurrenceState, ends: EndsChoice): RecurrenceState {
  return { ...state, ends };
}

/** The end-of-series row; each option keeps the other options' values. */
function EndsField({
  idPrefix,
  state,
  startsAt,
  disabled,
  onChange,
}: {
  idPrefix: string;
  state: RecurrenceState;
  startsAt: string;
  disabled: boolean;
  onChange: (next: EndsChoice) => void;
}): React.ReactElement {
  const name = `${idPrefix}-ends`;
  const options: Array<{ mode: EndsChoice["mode"]; label: string }> = [
    { mode: "never", label: "Never" },
    { mode: "on", label: "On date" },
    { mode: "after", label: "After" },
  ];
  return (
    <fieldset className="flex flex-wrap items-center gap-(--space-3)" aria-label="Ends">
      <legend className="text-body mb-(--space-2) font-medium text-(--text)">Ends</legend>
      {options.map((option) => (
        <label key={option.mode} className="text-body flex items-center gap-(--space-2) text-(--text)">
          <input
            type="radio"
            name={name}
            value={option.mode}
            checked={state.ends.mode === option.mode}
            disabled={disabled}
            onChange={() => {
              if (option.mode === "on") onChange({ mode: "on", date: defaultEndDate(startsAt) });
              else if (option.mode === "after") onChange({ mode: "after", count: DEFAULT_AFTER_COUNT });
              else onChange({ mode: "never" });
            }}
          />
          {option.label}
        </label>
      ))}
      {state.ends.mode === "on" ? (
        <input
          type="date"
          aria-label="End date"
          value={state.ends.date}
          disabled={disabled}
          className={controlClass}
          onChange={(event) => {
            if (state.ends.mode === "on") onChange({ mode: "on", date: event.target.value });
          }}
        />
      ) : null}
      {state.ends.mode === "after" ? (
        <span className="flex items-center gap-(--space-2)">
          <input
            type="number"
            min={1}
            aria-label="Number of occurrences"
            value={state.ends.count}
            disabled={disabled}
            className={cn(controlClass, "w-20")}
            onChange={(event) => onChange({ mode: "after", count: Number(event.target.value) })}
          />
          <span className="text-meta text-(--text-muted)">times</span>
        </span>
      ) : null}
    </fieldset>
  );
}

/** Monthly form: a day of the month, or the nth weekday of the month. */
function MonthlyField({
  state,
  disabled,
  onChange,
}: {
  state: RecurrenceState;
  disabled: boolean;
  onChange: (next: MonthlyChoice) => void;
}): React.ReactElement {
  const monthly = state.monthly;
  return (
    <div className="flex flex-wrap items-center gap-(--space-3)">
      <span className="text-body text-(--text-muted)">On</span>
      <select
        aria-label="Monthly pattern"
        value={monthly.on}
        disabled={disabled}
        className={controlClass}
        onChange={(event) =>
          onChange(withMonthlyMode(state, event.target.value === "weekday" ? "weekday" : "day").monthly)
        }
      >
        <option value="day">Day of the month</option>
        <option value="weekday">Weekday of the month</option>
      </select>
      {monthly.on === "day" ? (
        <span className="flex items-center gap-(--space-2)">
          <input
            type="number"
            min={1}
            max={31}
            aria-label="Day of the month"
            value={monthly.day}
            disabled={disabled}
            className={cn(controlClass, "w-20")}
            onChange={(event) => onChange({ on: "day", day: Number(event.target.value) })}
          />
          <span className="text-meta text-(--text-muted)">day</span>
        </span>
      ) : (
        <span className="flex items-center gap-(--space-2)">
          <select
            aria-label="Weekday ordinal"
            value={monthly.ordinal}
            disabled={disabled}
            className={controlClass}
            onChange={(event) => onChange({ ...monthly, ordinal: Number(event.target.value) })}
          >
            <option value={1}>First</option>
            <option value={2}>Second</option>
            <option value={3}>Third</option>
            <option value={4}>Fourth</option>
            <option value={5}>Fifth</option>
            <option value={-1}>Last</option>
          </select>
          <select
            aria-label="Weekday of the month"
            value={monthly.weekday}
            disabled={disabled}
            className={controlClass}
            onChange={(event) => onChange({ ...monthly, weekday: event.target.value as WeekdayCode })}
          >
            {WEEKDAY_CODES.map((code) => (
              <option key={code} value={code}>
                {WEEKDAY_LABELS[code]}
              </option>
            ))}
          </select>
        </span>
      )}
    </div>
  );
}

export function RecurrenceBuilder({
  idPrefix,
  value,
  startsAt,
  disabled = false,
  onChange,
}: RecurrenceBuilderProps): React.ReactElement {
  const parsed = React.useMemo(() => parseRecurrence(value, startsAt), [value, startsAt]);
  const state = parsed.state;
  // The raw editor stays open once it is shown for an inexpressible rule or
  // opened by hand; typing a rule back into the expressible shape must not
  // yank the field out from under the caret.
  const [rawOpen, setRawOpen] = React.useState(!parsed.expressible);
  const rawForced = !parsed.expressible;
  const rawVisible = rawOpen || rawForced;
  const ids = {
    repeat: `${idPrefix}-repeat`,
    raw: `${idPrefix}-rrule`,
    rawDisclosure: `${idPrefix}-raw`,
  };

  const emit = (next: RecurrenceState): void => onChange(serializeRecurrence(next));

  return (
    <div className="flex flex-col gap-(--space-4)">
      <div className="flex flex-wrap items-center gap-(--space-3)">
        <span id={`${idPrefix}-repeat-label`} className="text-body font-medium text-(--text)">
          Repeat every
        </span>
        {state.unit === "none" ? null : (
          <input
            id={ids.repeat}
            type="number"
            min={1}
            aria-label="Repeat interval"
            value={state.interval}
            disabled={disabled}
            className={cn(controlClass, "w-20")}
            onChange={(event) => emit({ ...state, interval: Number(event.target.value) })}
          />
        )}
        <select
          aria-label="Repeat unit"
          value={state.unit}
          disabled={disabled}
          className={controlClass}
          onChange={(event) => emit(withUnit(state, event.target.value as RepeatUnit))}
        >
          {REPEAT_UNITS.map((unit) => (
            <option key={unit} value={unit}>
              {UNIT_LABELS[unit]}
            </option>
          ))}
        </select>
      </div>

      {state.unit === "week" ? (
        <div role="group" aria-label="Repeat on" className="flex flex-wrap gap-(--space-2)">
          {WEEKDAY_CODES.map((code) => {
            const active = state.weekdays.includes(code);
            return (
              <button
                key={code}
                type="button"
                aria-pressed={active}
                aria-label={WEEKDAY_LABELS[code]}
                disabled={disabled}
                className={cn(
                  "text-meta size-9 rounded-full border border-(--border) font-medium text-(--text-muted)",
                  active ? "border-(--accent) bg-(--accent) text-(--bg)" : "bg-(--bg)",
                  FOCUS_RING
                )}
                onClick={() => emit(withWeekdayToggled(state, code))}
              >
                {WEEKDAY_LABELS[code].slice(0, 2)}
              </button>
            );
          })}
        </div>
      ) : null}

      {state.unit === "month" ? (
        <MonthlyField state={state} disabled={disabled} onChange={(next) => emit({ ...state, monthly: next })} />
      ) : null}

      {state.unit === "none" ? null : (
        <EndsField
          idPrefix={idPrefix}
          state={state}
          startsAt={startsAt}
          disabled={disabled}
          onChange={(next) => emit(changeEnds(state, next))}
        />
      )}

      {rawForced ? null : (
        <div>
          <button
            type="button"
            aria-expanded={rawVisible}
            aria-controls={ids.raw}
            disabled={disabled}
            className={cn("text-meta text-(--text-muted) underline-offset-2 hover:text-(--text)", FOCUS_RING)}
            onClick={() => setRawOpen((open) => !open)}
          >
            Advanced: rule text
          </button>
        </div>
      )}

      <div id={ids.rawDisclosure} hidden={!rawVisible}>
        <label htmlFor={ids.raw} className="text-body font-medium text-(--text)">
          Recurrence (RRULE)
        </label>
        <textarea
          id={ids.raw}
          aria-label="Recurrence (RRULE)"
          value={value}
          rows={2}
          disabled={disabled}
          spellCheck={false}
          placeholder="FREQ=WEEKLY;BYDAY=MO,WE,FR"
          className="text-body font-mono mt-(--space-2) min-h-14 w-full resize-y rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
          onChange={(event) => {
            setRawOpen(true);
            onChange(event.target.value);
          }}
        />
        <p className="text-meta mt-(--space-2) text-(--text-muted)">
          {rawForced
            ? "This stored rule uses a pattern the repeat controls cannot show, so it is edited as text. Leave it untouched to save it exactly as it is."
            : "Edit the RFC 5545 rule directly. An empty rule fires once at the start time."}
        </p>
      </div>
    </div>
  );
}
