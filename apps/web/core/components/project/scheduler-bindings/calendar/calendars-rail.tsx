/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useTranslation } from "@pi-dash/i18n";
import type { IScheduler } from "@pi-dash/services";
import { DEFAULT_SCHEDULER_COLOR } from "../constants";

type Props = {
  /** Workspace-scoped scheduler templates installed on this project. */
  schedulers: IScheduler[];
  /** Which schedulers are currently visible on the calendar. */
  isVisible: (schedulerId: string) => boolean;
  /** Toggle a scheduler's visibility. */
  onToggle: (schedulerId: string) => void;
  /** "Show all" — clears the hidden set. */
  onShowAll: () => void;
  /** "Hide all" — hides every scheduler. */
  onHideAll: () => void;
};

/**
 * The Google-Calendar-style "My calendars" rail. One row per scheduler that
 * has a binding on this project, with its color swatch + checkbox toggle.
 *
 * Hidden on mobile by the parent; here we render the desktop form only.
 */
export function CalendarsRail({ schedulers, isVisible, onToggle, onShowAll, onHideAll }: Props) {
  const { t } = useTranslation();

  if (schedulers.length === 0) {
    return (
      <aside className="hidden w-60 flex-shrink-0 border-l border-subtle p-4 md:block">
        <div className="text-13 font-medium text-primary">{t("Calendars")}</div>
        <p className="mt-2 text-12 text-secondary">{t("Install a scheduler from the List tab to see it here.")}</p>
      </aside>
    );
  }

  return (
    <aside className="hidden w-60 flex-shrink-0 border-l border-subtle md:block">
      <div className="flex items-center justify-between px-4 py-3">
        <div className="text-13 font-medium text-primary">{t("Calendars")}</div>
        <div className="flex items-center gap-1 text-12 text-secondary">
          <button type="button" onClick={onShowAll} className="hover:text-primary">
            {t("Show all")}
          </button>
          <span>/</span>
          <button type="button" onClick={onHideAll} className="hover:text-primary">
            {t("Hide all")}
          </button>
        </div>
      </div>
      <div className="max-h-[calc(100vh-12rem)] overflow-y-auto px-2 pb-2">
        {schedulers.map((s) => {
          const visible = isVisible(s.id);
          return (
            <label
              key={s.id}
              className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-13 hover:bg-layer-1"
            >
              <input
                type="checkbox"
                checked={visible}
                onChange={() => onToggle(s.id)}
                className="size-4 rounded border-subtle"
                style={{ accentColor: s.color || DEFAULT_SCHEDULER_COLOR }}
              />
              <span
                className="inline-block h-3 w-3 flex-shrink-0 rounded-sm"
                style={{ backgroundColor: s.color || DEFAULT_SCHEDULER_COLOR, opacity: visible ? 1 : 0.3 }}
              />
              <span className={visible ? "text-primary" : "text-tertiary"}>{s.name}</span>
            </label>
          );
        })}
      </div>
    </aside>
  );
}
