// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar visibility rail (parity/specs/agents.md §5, AGT-016). One
// checkbox row per installed definition in catalog order, with its color
// mark, plus show-all/hide-all. It only narrows what the grid draws: the
// occurrences read stays the same, and narrow screens drop the rail
// entirely because the grid needs the width.

import { Button } from "@pidash/kit";
import * as React from "react";

export interface RailCalendar {
  id: string;
  name: string;
  color: string;
}

export interface VisibilityRailProps {
  /** Installed definitions in catalog order. */
  calendars: readonly RailCalendar[];
  isVisible: (schedulerId: string) => boolean;
  onToggle: (schedulerId: string) => void;
  onShowAll: () => void;
  onHideAll: () => void;
}

export function VisibilityRail({
  calendars,
  isVisible,
  onToggle,
  onShowAll,
  onHideAll,
}: VisibilityRailProps): React.ReactElement {
  const inputId = React.useId();
  return (
    <aside
      aria-label="Calendars"
      className="hidden w-60 shrink-0 flex-col gap-(--space-3) overflow-y-auto border-r border-(--border) p-(--space-4) md:flex"
    >
      <div className="flex items-center justify-between gap-(--space-2)">
        <h2 className="text-meta font-medium tracking-wide text-(--text-muted) uppercase">Calendars</h2>
        <div className="flex items-center gap-(--space-1)">
          <Button size="small" variant="ghost" disabled={calendars.length === 0} onClick={onShowAll}>
            Show all
          </Button>
          <Button size="small" variant="ghost" disabled={calendars.length === 0} onClick={onHideAll}>
            Hide all
          </Button>
        </div>
      </div>
      {calendars.length === 0 ? (
        <p className="text-meta text-(--text-muted)">No schedulers are installed on this project.</p>
      ) : (
        <ul className="flex flex-col gap-(--space-1)">
          {calendars.map((calendar, index) => {
            const id = `${inputId}-${index}`;
            return (
              <li key={calendar.id}>
                <label
                  htmlFor={id}
                  className="flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-1) py-(--space-1) hover:bg-(--subtle)"
                >
                  <input
                    id={id}
                    type="checkbox"
                    className="size-3.5 accent-(--accent)"
                    checked={isVisible(calendar.id)}
                    onChange={() => onToggle(calendar.id)}
                  />
                  <span
                    aria-hidden="true"
                    className="inline-block size-2.5 shrink-0 rounded-sm"
                    style={{ backgroundColor: calendar.color }}
                  />
                  <span className="text-meta truncate text-(--text)">{calendar.name}</span>
                </label>
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );
}

/**
 * The rail's rows from the project's installs, ordered the way the
 * workspace catalog lists them; installs whose definition is missing from
 * the catalog keep their server order at the end.
 */
export function railCalendars(
  bindings: readonly { scheduler: string; scheduler_name: string; scheduler_color: string }[],
  catalogOrder: readonly string[]
): RailCalendar[] {
  const rank = new Map(catalogOrder.map((id, index) => [id, index]));
  return bindings
    .map((binding) => ({
      id: binding.scheduler,
      name: binding.scheduler_name,
      color: binding.scheduler_color,
    }))
    .sort((left, right) => {
      const leftRank = rank.get(left.id) ?? Number.MAX_SAFE_INTEGER;
      const rightRank = rank.get(right.id) ?? Number.MAX_SAFE_INTEGER;
      return leftRank - rightRank;
    });
}
