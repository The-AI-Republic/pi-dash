// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Expiry controls for token creation (parity/specs/workspace-settings.md §26,
// WSET-026): a preset select, a custom calendar floored at tomorrow, the
// never-expires toggle, and the live expiry preview. The select is the first
// action in the dialog by design, so keyboard users reach the expiry rule
// before the submit controls.

import { Popover, Switch } from "@pidash/kit";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import * as React from "react";

import {
  earliestCustomDay,
  EXPIRY_PRESETS,
  expiryLine,
  localIsoDay,
  resolveExpiry,
  type ExpiryChoice,
  type TokenDraft,
} from "./model.js";

const MONTH_LABELS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
] as const;

const WEEKDAY_LABELS = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] as const;

interface CalendarCell {
  iso: string;
  day: number;
  inMonth: boolean;
  disabled: boolean;
}

function isoFor(year: number, month0: number, day: number): string {
  return `${year}-${String(month0 + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

/** One six-week month grid with the earliest allowed day marked disabled. */
export function expiryMonthGrid(year: number, month0: number, minDay: string): CalendarCell[][] {
  const lead = new Date(year, month0, 1).getDay();
  const cells: CalendarCell[] = [];
  for (let index = 0; index < 42; index++) {
    const date = new Date(year, month0, 1 - lead + index);
    const iso = isoFor(date.getFullYear(), date.getMonth(), date.getDate());
    cells.push({
      iso,
      day: date.getDate(),
      inMonth: date.getMonth() === month0,
      disabled: iso < minDay,
    });
  }
  return Array.from({ length: 6 }, (_, week) => cells.slice(week * 7, week * 7 + 7));
}

function ordinal(day: number): string {
  if (day % 100 >= 11 && day % 100 <= 13) return `${day}th`;
  switch (day % 10) {
    case 1:
      return `${day}st`;
    case 2:
      return `${day}nd`;
    case 3:
      return `${day}rd`;
    default:
      return `${day}th`;
  }
}

/** Accessible day name, e.g. `Tuesday, October 20th, 2026`. */
export function accessibleDayName(iso: string): string {
  const [year, month, day] = iso.split("-").map(Number) as [number, number, number];
  const date = new Date(year, month - 1, day);
  const weekday = date.toLocaleString("en-US", { weekday: "long" });
  return `${weekday}, ${MONTH_LABELS[month - 1]} ${ordinal(day)}, ${year}`;
}

export interface TokenExpiryCalendarProps {
  value: string | null;
  minDay: string;
  onSelect: (iso: string) => void;
  now?: Date | undefined;
}

/** Month calendar for the custom expiry day; days are grid cells with buttons. */
export function TokenExpiryCalendar({
  value,
  minDay,
  onSelect,
  now = new Date(),
}: TokenExpiryCalendarProps): React.ReactElement {
  const [visible, setVisible] = React.useState(() => {
    const anchor = value ?? localIsoDay(now);
    const [year, month] = anchor.split("-").map(Number) as [number, number];
    return { year, month0: month - 1 };
  });
  const weeks = React.useMemo(
    () => expiryMonthGrid(visible.year, visible.month0, minDay),
    [visible.year, visible.month0, minDay]
  );
  const shift = (delta: number): void => {
    const total = visible.year * 12 + visible.month0 + delta;
    setVisible({ year: Math.floor(total / 12), month0: ((total % 12) + 12) % 12 });
  };

  return (
    <div data-testid="token-expiry-calendar" className="flex flex-col gap-(--space-3)">
      <div className="flex items-center justify-between gap-(--space-2)">
        <button
          type="button"
          aria-label="Go to the Previous Month"
          onClick={() => shift(-1)}
          className="rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <ChevronLeft size={14} strokeWidth={2} aria-hidden="true" />
        </button>
        <span className="text-emphasis text-(--text)">
          {MONTH_LABELS[visible.month0]} {visible.year}
        </span>
        <button
          type="button"
          aria-label="Go to the Next Month"
          onClick={() => shift(1)}
          className="rounded-(--radius-control) p-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <ChevronRight size={14} strokeWidth={2} aria-hidden="true" />
        </button>
      </div>
      <div className="grid grid-cols-7 gap-(--space-1)">
        {WEEKDAY_LABELS.map((label) => (
          <span key={label} className="text-caption grid place-items-center text-(--text-muted)">
            {label}
          </span>
        ))}
      </div>
      <div role="grid" aria-label="Expiry date" className="grid grid-cols-7 gap-(--space-1)">
        {weeks.flat().map((cell) => (
          <div key={cell.iso} role="gridcell" className="grid place-items-center">
            <button
              type="button"
              aria-label={accessibleDayName(cell.iso)}
              aria-pressed={cell.iso === value}
              disabled={cell.disabled}
              onClick={() => onSelect(cell.iso)}
              data-in-month={cell.inMonth ? "true" : "false"}
              className={
                cell.iso === value
                  ? "text-meta size-(--space-6) rounded-(--radius-control) bg-(--accent) text-(--on-accent) disabled:cursor-not-allowed disabled:opacity-40"
                  : cell.inMonth
                    ? "text-meta size-(--space-6) rounded-(--radius-control) text-(--text) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-40"
                    : "text-meta size-(--space-6) rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) disabled:cursor-not-allowed disabled:opacity-40"
              }
            >
              {cell.day}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function presetLabel(choice: ExpiryChoice): string {
  if (choice === null) return "Set expiration";
  if (choice === "custom") return "Custom date";
  return EXPIRY_PRESETS.find((preset) => preset.key === choice)?.label ?? "Set expiration";
}

export interface TokenExpiryControlsProps {
  draft: TokenDraft;
  onChange: (next: TokenDraft) => void;
  now?: Date | undefined;
  timeZone?: string | undefined;
}

/** The complete expiry block: select, custom day, toggle and preview. */
export function TokenExpiryControls({
  draft,
  onChange,
  now = new Date(),
  timeZone = "UTC",
}: TokenExpiryControlsProps): React.ReactElement {
  const [presetOpen, setPresetOpen] = React.useState(false);
  const [customOpen, setCustomOpen] = React.useState(false);
  const minDay = earliestCustomDay(now);
  const expiry = resolveExpiry(draft, now);

  return (
    <div className="flex flex-col gap-(--space-4)">
      <Popover
        open={presetOpen}
        onOpenChange={setPresetOpen}
        trigger={
          <button
            type="button"
            disabled={draft.neverExpires}
            aria-haspopup="listbox"
            className="text-body inline-flex h-(--control-height) w-fit items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
          >
            {presetLabel(draft.choice)}
          </button>
        }
      >
        <div role="listbox" aria-label="Expiry" className="flex flex-col">
          {EXPIRY_PRESETS.map((preset) => (
            <button
              key={preset.key}
              type="button"
              role="option"
              aria-selected={draft.choice === preset.key}
              onClick={() => {
                onChange({ ...draft, choice: preset.key });
                setPresetOpen(false);
              }}
              className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
            >
              {preset.label}
            </button>
          ))}
        </div>
      </Popover>

      {draft.choice === "custom" ? (
        <Popover
          open={customOpen}
          onOpenChange={setCustomOpen}
          trigger={
            <button
              type="button"
              disabled={draft.neverExpires}
              className="text-body inline-flex h-(--control-height) w-fit items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) disabled:cursor-not-allowed disabled:opacity-50"
            >
              <CalendarDays size={14} strokeWidth={2} aria-hidden="true" />
              {draft.customDay === null ? "Set date" : draft.customDay}
            </button>
          }
        >
          <TokenExpiryCalendar
            value={draft.customDay}
            minDay={minDay}
            now={now}
            onSelect={(iso) => {
              onChange({ ...draft, customDay: iso });
              setCustomOpen(false);
            }}
          />
        </Popover>
      ) : null}

      <div className="flex items-center justify-between gap-(--space-4)">
        <button
          type="button"
          className="text-body text-(--text)"
          onClick={() => onChange({ ...draft, neverExpires: !draft.neverExpires })}
        >
          Never expires
        </button>
        <Switch
          label="Never expires"
          checked={draft.neverExpires}
          onCheckedChange={(neverExpires) => onChange({ ...draft, neverExpires })}
        />
      </div>

      {expiry === undefined || draft.neverExpires ? null : (
        <p className="text-meta text-(--text-muted)">{expiryLine(expiry, timeZone)}</p>
      )}
    </div>
  );
}
