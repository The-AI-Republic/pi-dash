/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@pi-dash/i18n";
import { TextArea } from "@pi-dash/ui";
import type { EndsMode, RecurrenceDraft, RecurrenceUnit } from "./rrule-builder";
import { defaultDraft, parseRrule, serializeRrule } from "./rrule-builder";

type Props = {
  id: string;
  /** The bare RRULE string held by the form ("" = single-shot). */
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  /** Current dtstart form value — seeds weekday/monthday defaults. */
  dtstart: string;
  hasError?: boolean;
};

const INPUT_CLS =
  "rounded-md border border-subtle bg-surface-1 px-2 py-1.5 text-13 text-primary focus:ring-1 focus:ring-accent-strong focus:outline-none";

/** Chip display order: Sunday first, Google-Calendar style. Values are rrule weekday numbers (0=MO … 6=SU). */
const CHIP_ORDER = [6, 0, 1, 2, 3, 4, 5];

// "" stays NaN in the draft so a cleared field doesn't snap back mid-typing;
// serializeRrule clamps NaN/0 to the INTERVAL/COUNT >= 1 the server demands.
function parsePositiveInt(raw: string): number {
  if (raw === "") return NaN;
  const n = parseInt(raw, 10);
  return Number.isNaN(n) ? NaN : Math.max(1, n);
}

/**
 * Google-Calendar-style recurrence builder over a raw RRULE form field.
 *
 * The form still stores the plain RRULE string; this component parses it
 * into widget state and re-serializes on widget interaction only. A rule
 * the widget cannot express keeps the stored string untouched and forces
 * the raw textarea open, so an unrelated edit never rewrites it.
 */
export function RecurrenceEditor({ id, value, onChange, onBlur, dtstart, hasError }: Props) {
  const { t } = useTranslation();

  const [draft, setDraft] = useState<RecurrenceDraft | null>(() => parseRrule(value, dtstart));
  const [rawOpen, setRawOpen] = useState<boolean>(() => parseRrule(value, dtstart) === null);
  // Last string the widget itself emitted — external value changes (modal
  // reset, raw typing) re-parse; our own emissions must not clobber the
  // sub-state the widget remembers (e.g. chips kept while switching units).
  const lastEmitted = useRef<string | null>(null);

  useEffect(() => {
    if (lastEmitted.current !== null && value === lastEmitted.current) return;
    const parsed = parseRrule(value, dtstart);
    setDraft(parsed);
    if (parsed === null) setRawOpen(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  const weekdayNames = [
    t("Monday"),
    t("Tuesday"),
    t("Wednesday"),
    t("Thursday"),
    t("Friday"),
    t("Saturday"),
    t("Sunday"),
  ];
  const ordinalNames: Record<number, string> = {
    1: t("first"),
    2: t("second"),
    3: t("third"),
    4: t("fourth"),
    [-1]: t("last"),
  };

  const apply = (next: RecurrenceDraft) => {
    setDraft(next);
    const serialized = serializeRrule(next);
    lastEmitted.current = serialized;
    onChange(serialized);
  };

  const update = (patch: Partial<RecurrenceDraft>) => {
    apply({ ...(draft ?? defaultDraft(dtstart)), ...patch });
  };

  const handleUnitChange = (unit: RecurrenceUnit) => {
    const base = draft ?? defaultDraft(dtstart);
    // Re-derive the unit-specific sub-state from the current dtstart so the
    // widget lands on "what the start date implies", like Google Calendar.
    const fresh = defaultDraft(dtstart);
    if (unit === "WEEKLY") {
      apply({ ...base, unit, weekdays: fresh.weekdays });
    } else if (unit === "MONTHLY") {
      apply({
        ...base,
        unit,
        monthlyMode: fresh.monthlyMode,
        monthday: fresh.monthday,
        nth: fresh.nth,
        nthWeekday: fresh.nthWeekday,
      });
    } else {
      apply({ ...base, unit });
    }
  };

  const toggleWeekday = (weekday: number) => {
    if (!draft) return;
    const has = draft.weekdays.includes(weekday);
    // Never allow zero chips — a weekly rule needs at least one day.
    if (has && draft.weekdays.length === 1) return;
    const weekdays = has ? draft.weekdays.filter((d) => d !== weekday) : [...draft.weekdays, weekday];
    update({ weekdays });
  };

  const handleEndsChange = (ends: EndsMode) => {
    if (ends === "until" && !draft?.untilDate) {
      // Prefill with ~3 months out so picking "On" is one click, not two.
      const base = new Date();
      base.setUTCDate(base.getUTCDate() + 90);
      update({ ends, untilDate: base.toISOString().slice(0, 10) });
      return;
    }
    update({ ends });
  };

  const unitValue: RecurrenceUnit | "" = draft?.unit ?? "";

  return (
    <div className="flex flex-col gap-3 rounded-md border border-subtle p-3">
      {draft === null ? (
        <p className="text-12 text-secondary">
          {t(
            "This rule uses features the builder can't edit, so it opens as raw RRULE. Editing below keeps it untouched otherwise."
          )}
        </p>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <label htmlFor={`${id}-unit`} className="text-13 text-primary">
              {t("Repeat every")}
            </label>
            {draft.unit !== "NONE" && (
              <input
                type="number"
                min={1}
                aria-label={t("Interval")}
                className={`${INPUT_CLS} w-16`}
                value={Number.isNaN(draft.interval) ? "" : draft.interval}
                onChange={(e) => update({ interval: parsePositiveInt(e.target.value) })}
              />
            )}
            <select
              id={`${id}-unit`}
              className={INPUT_CLS}
              value={unitValue}
              onChange={(e) => handleUnitChange(e.target.value as RecurrenceUnit)}
            >
              <option value="DAILY">{t("day")}</option>
              <option value="WEEKLY">{t("week")}</option>
              <option value="MONTHLY">{t("month")}</option>
              <option value="YEARLY">{t("year")}</option>
              <option value="NONE">{t("(does not repeat)")}</option>
            </select>
          </div>

          {draft.unit === "WEEKLY" && (
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-13 text-primary">{t("Repeat on")}</span>
              <div className="flex items-center gap-1" role="group" aria-label={t("Repeat on")}>
                {CHIP_ORDER.map((weekday) => {
                  const active = draft.weekdays.includes(weekday);
                  return (
                    <button
                      key={weekday}
                      type="button"
                      aria-pressed={active}
                      aria-label={weekdayNames[weekday]}
                      onClick={() => toggleWeekday(weekday)}
                      className={`h-7 w-7 rounded-full text-12 font-medium transition-colors ${
                        active ? "bg-accent-primary text-on-color" : "bg-layer-1 text-secondary hover:text-primary"
                      }`}
                    >
                      {weekdayNames[weekday].charAt(0)}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {draft.unit === "MONTHLY" && (
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label={t("Monthly repeat mode")}
                className={INPUT_CLS}
                value={draft.monthlyMode}
                onChange={(e) => update({ monthlyMode: e.target.value === "weekday" ? "weekday" : "monthday" })}
              >
                <option value="monthday">{t("Monthly on day {day}", { day: draft.monthday })}</option>
                <option value="weekday">
                  {t("Monthly on the {nth} {weekday}", {
                    nth: ordinalNames[draft.nth] ?? String(draft.nth),
                    weekday: weekdayNames[draft.nthWeekday],
                  })}
                </option>
              </select>
            </div>
          )}

          {draft.unit !== "NONE" && (
            <fieldset className="flex flex-col gap-1.5">
              <legend className="mb-1 text-13 text-primary">{t("Ends")}</legend>
              <label className="flex items-center gap-2 text-13 text-primary">
                <input
                  type="radio"
                  name={`${id}-ends`}
                  checked={draft.ends === "never"}
                  onChange={() => handleEndsChange("never")}
                />
                {t("Never")}
              </label>
              <label className="flex items-center gap-2 text-13 text-primary">
                <input
                  type="radio"
                  name={`${id}-ends`}
                  checked={draft.ends === "until"}
                  onChange={() => handleEndsChange("until")}
                />
                {t("On")}
                <input
                  type="date"
                  aria-label={t("End date (UTC)")}
                  className={INPUT_CLS}
                  value={draft.untilDate}
                  disabled={draft.ends !== "until"}
                  onChange={(e) => update({ ends: "until", untilDate: e.target.value })}
                />
                <span className="text-12 text-secondary">{t("(UTC, inclusive)")}</span>
              </label>
              <label className="flex items-center gap-2 text-13 text-primary">
                <input
                  type="radio"
                  name={`${id}-ends`}
                  checked={draft.ends === "count"}
                  onChange={() => handleEndsChange("count")}
                />
                {t("After")}
                <input
                  type="number"
                  min={1}
                  aria-label={t("Number of occurrences")}
                  className={`${INPUT_CLS} w-20`}
                  value={Number.isNaN(draft.count) ? "" : draft.count}
                  disabled={draft.ends !== "count"}
                  onChange={(e) => update({ ends: "count", count: parsePositiveInt(e.target.value) })}
                />
                {t("occurrences")}
              </label>
            </fieldset>
          )}

          <button
            type="button"
            className="self-start text-12 font-medium text-secondary hover:text-primary hover:underline"
            onClick={() => setRawOpen((v) => !v)}
          >
            {rawOpen ? t("Hide raw RRULE") : t("Advanced: edit raw RRULE")}
          </button>
        </>
      )}

      {(rawOpen || draft === null) && (
        <div className="flex flex-col gap-1">
          <TextArea
            id={id}
            name={id}
            className="min-h-[56px] text-13"
            placeholder="FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"
            hasError={!!hasError}
            value={value}
            onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => onChange(e.target.value)}
            onBlur={onBlur}
          />
          <p className="text-12 text-secondary">
            {t(
              "RFC 5545 RRULE — e.g. ``FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR``. Leave blank to fire only once at the start."
            )}
          </p>
        </div>
      )}
    </div>
  );
}
