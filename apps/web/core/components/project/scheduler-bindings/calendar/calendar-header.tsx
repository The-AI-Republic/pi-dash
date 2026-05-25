/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { cn } from "@pi-dash/utils";
import { addMonths, addWeeks, formatMonthYear, type CalendarView } from "./date-helpers";

type Props = {
  view: CalendarView;
  viewDate: Date;
  onChangeView: (view: CalendarView) => void;
  onChangeDate: (date: Date) => void;
};

export function SchedulerCalendarHeader({ view, viewDate, onChangeView, onChangeDate }: Props) {
  const { t } = useTranslation();

  const step = view === "week" ? 1 : 1;
  const stepFn = view === "week" ? addWeeks : addMonths;

  return (
    <div className="flex items-center justify-between gap-2 border-b border-subtle px-4 py-2">
      <div className="flex items-center gap-1">
        <button
          type="button"
          onClick={() => onChangeDate(new Date())}
          className="rounded-md border border-subtle px-2.5 py-1 text-13 text-primary hover:bg-layer-1"
        >
          {t("scheduler_bindings.calendar.today")}
        </button>
        <button
          type="button"
          onClick={() => onChangeDate(stepFn(viewDate, -step))}
          aria-label="Previous"
          className="rounded-md p-1 text-secondary hover:bg-layer-1 hover:text-primary"
        >
          <ChevronLeft className="size-4" />
        </button>
        <button
          type="button"
          onClick={() => onChangeDate(stepFn(viewDate, step))}
          aria-label="Next"
          className="rounded-md p-1 text-secondary hover:bg-layer-1 hover:text-primary"
        >
          <ChevronRight className="size-4" />
        </button>
        <div className="ml-2 text-14 font-medium text-primary">{formatMonthYear(viewDate)}</div>
      </div>

      <div className="flex items-center gap-1 rounded-md border border-subtle p-0.5">
        {(["month", "week"] as const).map((v) => (
          <button
            key={v}
            type="button"
            onClick={() => onChangeView(v)}
            className={cn(
              "rounded-sm px-2.5 py-1 text-13 transition-colors",
              view === v ? "bg-layer-2 text-primary" : "text-secondary hover:text-primary"
            )}
          >
            {v === "month" ? t("scheduler_bindings.calendar.month") : t("scheduler_bindings.calendar.week")}
          </button>
        ))}
      </div>
    </div>
  );
}
