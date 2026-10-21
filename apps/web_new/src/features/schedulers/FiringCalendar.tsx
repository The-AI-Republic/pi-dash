// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project firing calendar (parity/specs/agents.md §5, AGT-015–017, 062).
// The calendar tab's body: a header that switches between the week and
// month grids, steps a period and jumps home, the visibility rail beside
// the grid, and the occurrence panel a block opens. Reads happen once per
// visible window through the occurrences contract; the rail narrows what
// is drawn without touching the read. Nothing here drags, creates,
// reschedules or exports.

import { useQueryClient } from "@tanstack/react-query";
import type { SchedulerBinding, SchedulerOccurrence } from "@pidash/api-client";
import { Button, Skeleton } from "@pidash/kit";
import { ChevronLeft, ChevronRight } from "lucide-react";
import * as React from "react";

import {
  addDays,
  formatMonthYear,
  monthWindow,
  nextMonth,
  occurrencesByDay,
  previousMonth,
  weekWindow,
} from "./calendarModel.js";
import { CalendarMonthView } from "./CalendarMonthView.js";
import { CalendarWeekView } from "./CalendarWeekView.js";
import { TRUNCATION_NOTICE, occurrenceKeys, useSchedulerOccurrences } from "./occurrences.js";
import { OccurrencePanel } from "./OccurrencePanel.js";
import { useSchedulerBindings } from "./installData.js";
import { useSchedulers } from "./api.js";
import { useSchedulerVisibility } from "./visibility.js";
import { VisibilityRail, railCalendars, type RailCalendar } from "./VisibilityRail.js";

export type CalendarView = "week" | "month";

export interface FiringCalendarProps {
  workspaceSlug: string;
  projectId: string;
  /** Project admins get the panel's edit entry point. */
  canEdit: boolean;
  /** Clock source, injectable so tests can pin "now". */
  now?: Date | undefined;
}

// The edit dialog brings the full recurrence builder with it. It only
// matters once an admin picks Edit, so the panel loads it on demand and
// the calendar's first paint stays light.
const EditInstallDialog = React.lazy(() =>
  import("./EditInstallDialog.js").then((module) => ({ default: module.EditInstallDialog }))
);

/** How often the live time line and today mark re-read the clock. */
export const CLOCK_TICK_MS = 30_000;

function useTickingClock(pinned: Date | undefined): Date {
  const [now, setNow] = React.useState<Date>(() => (pinned === undefined ? new Date() : new Date(pinned.getTime())));
  React.useEffect(() => {
    if (pinned !== undefined) {
      setNow(new Date(pinned.getTime()));
      return;
    }
    const timer = window.setInterval(() => setNow(new Date()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, [pinned]);
  return now;
}

/** Selects the start anchor when the view changes, so each view opens on today. */
function anchorFor(view: CalendarView, now: Date): Date {
  return view === "week" ? new Date(now) : new Date(now.getFullYear(), now.getMonth(), 1);
}

export function FiringCalendar({
  workspaceSlug,
  projectId,
  canEdit,
  now: pinned,
}: FiringCalendarProps): React.ReactElement {
  const queryClient = useQueryClient();
  const now = useTickingClock(pinned);
  const [view, setView] = React.useState<CalendarView>("week");
  const [anchor, setAnchor] = React.useState<Date>(() => anchorFor("week", now));
  const [selected, setSelected] = React.useState<SchedulerOccurrence | null>(null);
  const [editing, setEditing] = React.useState<SchedulerBinding | null>(null);

  // The route can change project without remounting this component. Drop a
  // panel or dialog that pointed at the previous project before reading the
  // next project's installs.
  React.useEffect(() => {
    setSelected(null);
    setEditing(null);
  }, [workspaceSlug, projectId]);

  const bindings = useSchedulerBindings(workspaceSlug, projectId);
  const catalog = useSchedulers(workspaceSlug);
  const visibility = useSchedulerVisibility(workspaceSlug, projectId);
  const window = view === "week" ? weekWindow(anchor) : monthWindow(anchor);
  const occurrences = useSchedulerOccurrences(workspaceSlug, projectId, window);

  const installed = React.useMemo(() => bindings.data ?? [], [bindings.data]);
  const calendars = React.useMemo<RailCalendar[]>(
    () =>
      railCalendars(
        installed,
        (catalog.data ?? []).map((definition) => definition.id)
      ),
    [installed, catalog.data]
  );

  const visibleOccurrences = React.useMemo(
    () => (occurrences.data?.occurrences ?? []).filter((occurrence) => visibility.isVisible(occurrence.scheduler_id)),
    [occurrences.data, visibility]
  );
  const byDay = React.useMemo(() => occurrencesByDay(visibleOccurrences), [visibleOccurrences]);
  const truncated = occurrences.data?.has_more ?? false;

  const selectedBinding = selected === null ? null : (installed.find((row) => row.id === selected.binding_id) ?? null);

  function switchView(next: CalendarView): void {
    setSelected(null);
    setView(next);
  }

  function step(direction: "prev" | "next"): void {
    setAnchor((current) => {
      if (view === "week") return addDays(current, direction === "next" ? 7 : -7);
      return direction === "next" ? nextMonth(current) : previousMonth(current);
    });
  }

  function jumpToToday(): void {
    setAnchor(anchorFor(view, now));
  }

  function openEdit(binding: SchedulerBinding): void {
    setSelected(null);
    setEditing(binding);
  }

  function settleAfterEdit(): void {
    void queryClient.invalidateQueries({ queryKey: occurrenceKeys.all(workspaceSlug, projectId) });
    void bindings.refetch();
  }

  const empty = bindings.isSuccess && installed.length === 0;

  return (
    <section aria-label="Firing calendar" className="relative flex h-full min-h-0 w-full">
      <VisibilityRail
        calendars={calendars}
        isVisible={visibility.isVisible}
        onToggle={visibility.toggle}
        onShowAll={visibility.showAll}
        onHideAll={() => visibility.hideAll(calendars.map((calendar) => calendar.id))}
      />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <header className="flex flex-wrap items-center justify-between gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2)">
          <div className="flex items-center gap-(--space-2)">
            <Button size="small" onClick={jumpToToday}>
              Today
            </Button>
            <Button size="small" variant="ghost" aria-label="Previous" onClick={() => step("prev")}>
              <ChevronLeft size={16} strokeWidth={1.5} aria-hidden="true" />
              Previous
            </Button>
            <Button size="small" variant="ghost" aria-label="Next" onClick={() => step("next")}>
              Next
              <ChevronRight size={16} strokeWidth={1.5} aria-hidden="true" />
            </Button>
            <div className="text-body font-medium text-(--text)">{formatMonthYear(anchor)}</div>
          </div>
          <div className="flex items-center gap-(--space-2)">
            {truncated ? <span className="text-meta text-(--text-muted)">{TRUNCATION_NOTICE}</span> : null}
            <div
              className="flex items-center rounded-(--radius-control) border border-(--border)"
              role="group"
              aria-label="Calendar view"
            >
              <Button size="small" variant={view === "week" ? "secondary" : "ghost"} onClick={() => switchView("week")}>
                Week
              </Button>
              <Button
                size="small"
                variant={view === "month" ? "secondary" : "ghost"}
                onClick={() => switchView("month")}
              >
                Month
              </Button>
            </div>
          </div>
        </header>
        <div className="min-h-0 flex-1 overflow-hidden">
          {occurrences.isPending && bindings.isPending ? (
            <div role="status" aria-label="Loading calendar" className="flex flex-col gap-(--space-3) p-(--space-6)">
              <Skeleton width="220px" height="16px" />
              <Skeleton width="100%" height="240px" />
            </div>
          ) : empty ? (
            <div className="flex h-full flex-col items-center justify-center gap-(--space-2) p-(--space-8)">
              <h2 className="text-h3 text-(--text)">No schedulers installed</h2>
              <p className="text-body text-(--text-muted)">
                Install a scheduler on this project to see when it fires here.
              </p>
            </div>
          ) : view === "week" ? (
            <CalendarWeekView anchor={anchor} now={now} byDay={byDay} onSelect={setSelected} />
          ) : (
            <CalendarMonthView anchor={anchor} now={now} byDay={byDay} truncated={truncated} onSelect={setSelected} />
          )}
        </div>
      </div>
      {selected === null ? null : (
        <OccurrencePanel
          occurrence={selected}
          binding={selectedBinding}
          installHref={`/${workspaceSlug}/projects/${projectId}/schedulers/${selected.binding_id}`}
          canEdit={canEdit}
          onClose={() => setSelected(null)}
          onEdit={openEdit}
          workspaceSlug={workspaceSlug}
          projectId={projectId}
        />
      )}
      {editing === null ? null : (
        <React.Suspense fallback={null}>
          <EditInstallDialog
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            binding={editing}
            onClose={() => setEditing(null)}
            onSaved={settleAfterEdit}
          />
        </React.Suspense>
      )}
    </section>
  );
}
