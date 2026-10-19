// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Schedule editor shared by the install dialogs (parity/specs/agents.md
// §2, AGT-006/008/009). Plain fields: start, zone, rule, context and the
// enabled flag, with the rule's plain-language sentence beneath it. The
// recurrence builder of NEWFRONT-587 wraps this same value shape.

import { Input, Switch, Textarea } from "@pidash/kit";
import * as React from "react";

import {
  DEFAULT_TZID,
  describeRrule,
  hasScheduleErrors,
  schedulePayload,
  timeZoneOptions,
  validateSchedule,
  type ScheduleErrors,
  type ScheduleValue,
} from "./schedule.js";

export interface ScheduleFieldsProps {
  /** Field id prefix, so two mounted dialogs never share ids. */
  idPrefix: string;
  value: ScheduleValue;
  errors: ScheduleErrors;
  disabled?: boolean;
  onChange: <K extends keyof ScheduleValue>(field: K, next: ScheduleValue[K]) => void;
}

/** Ids the shared fields and their labels are wired through. */
export function scheduleFieldIds(idPrefix: string) {
  return {
    startsAt: `${idPrefix}-starts-at`,
    timeZone: `${idPrefix}-time-zone`,
    rrule: `${idPrefix}-rrule`,
    projectContext: `${idPrefix}-project-context`,
  };
}

export function ScheduleFields({
  idPrefix,
  value,
  errors,
  disabled = false,
  onChange,
}: ScheduleFieldsProps): React.ReactElement {
  const ids = scheduleFieldIds(idPrefix);
  const zones = React.useMemo(() => timeZoneOptions(), []);
  const sentence = React.useMemo(() => describeRrule(value.rrule), [value.rrule]);
  return (
    <div className="flex flex-col gap-(--space-5)">
      <div className="grid grid-cols-1 gap-(--space-4) md:grid-cols-2">
        <Input
          id={ids.startsAt}
          label="Starts at"
          type="datetime-local"
          value={value.startsAt}
          disabled={disabled}
          hint="The first firing. The rule expands from this moment."
          {...(errors.startsAt === undefined ? {} : { error: errors.startsAt })}
          onChange={(event) => onChange("startsAt", event.target.value)}
        />
        <div className="flex flex-col gap-(--space-2)">
          <label htmlFor={ids.timeZone} className="text-body font-medium text-(--text)">
            Time zone
          </label>
          <select
            id={ids.timeZone}
            value={value.timeZone}
            disabled={disabled}
            className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
            onChange={(event) => onChange("timeZone", event.target.value || DEFAULT_TZID)}
          >
            {zones.map((zone) => (
              <option key={zone} value={zone}>
                {zone}
              </option>
            ))}
          </select>
          <p className="text-meta text-(--text-muted)">
            Stored with the install so wall-clock times keep their meaning.
          </p>
        </div>
      </div>

      <div className="flex flex-col gap-(--space-2)">
        <label htmlFor={ids.rrule} className="text-body font-medium text-(--text)">
          Recurrence (RRULE)
        </label>
        <textarea
          id={ids.rrule}
          value={value.rrule}
          rows={2}
          disabled={disabled}
          spellCheck={false}
          placeholder="FREQ=WEEKLY;BYDAY=MO,WE,FR;BYHOUR=9"
          className="text-body font-mono min-h-14 w-full resize-y rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none disabled:cursor-not-allowed disabled:opacity-50"
          onChange={(event) => onChange("rrule", event.target.value)}
        />
        <p className="text-meta text-(--text-muted)">
          RFC 5545 rule, for example <code>FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR</code>. Leave it blank to fire only once at
          the start. <span className="text-(--text)">{sentence}.</span>
        </p>
      </div>

      <Textarea
        id={ids.projectContext}
        label="Project context (optional)"
        value={value.projectContext}
        rows={3}
        disabled={disabled}
        hint="Appended to the scheduler's base prompt when it fires on this project."
        onChange={(event) => onChange("projectContext", event.target.value)}
      />

      <div className="flex items-center justify-between gap-(--space-4)">
        <div>
          <p className="text-body font-medium text-(--text)">Enabled</p>
          <p className="text-meta text-(--text-muted)">A disabled install stays on the project but never fires.</p>
        </div>
        <Switch
          label={value.enabled ? "Disable this install" : "Enable this install"}
          checked={value.enabled}
          disabled={disabled}
          onCheckedChange={(checked) => onChange("enabled", checked)}
        />
      </div>
    </div>
  );
}

/** The schedule draft plus its inline errors, shared by both dialogs. */
export function useScheduleDraft(initial: ScheduleValue): {
  value: ScheduleValue;
  errors: ScheduleErrors;
  change: <K extends keyof ScheduleValue>(field: K, next: ScheduleValue[K]) => void;
  validate: () => boolean;
  payload: () => ReturnType<typeof schedulePayload>;
} {
  const [value, setValue] = React.useState(initial);
  const [errors, setErrors] = React.useState<ScheduleErrors>({});
  const change = React.useCallback(<K extends keyof ScheduleValue>(field: K, next: ScheduleValue[K]) => {
    setValue((current) => ({ ...current, [field]: next }));
    setErrors((current) => ({ ...current, [field]: undefined }));
  }, []);
  const validate = React.useCallback(() => {
    const next = validateSchedule(value);
    setErrors(next);
    return !hasScheduleErrors(next);
  }, [value]);
  const payload = React.useCallback(() => schedulePayload(value), [value]);
  return { value, errors, change, validate, payload };
}
