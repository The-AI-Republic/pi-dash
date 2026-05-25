/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo, useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { useTranslation } from "@pi-dash/i18n";
import { Spinner } from "@pi-dash/ui";
import type { IScheduler, ISchedulerBinding, ISchedulerOccurrence } from "@pi-dash/services";
import { SchedulerService } from "@pi-dash/services";
import { EditSchedulerBindingModal } from "@/components/project/scheduler-bindings/edit-binding-modal";
import { CalendarsRail } from "./calendars-rail";
import { SchedulerCalendarHeader } from "./calendar-header";
import { type CalendarView, windowForView } from "./date-helpers";
import { SchedulerMonthView } from "./month-view";
import { OccurrenceDrawer } from "./occurrence-drawer";
import { useSchedulerOccurrences } from "./use-occurrences";
import { useVisibleSchedulers } from "./use-visible-schedulers";
import { SchedulerWeekView } from "./week-view";

type Props = {
  workspaceSlug: string;
  projectId: string;
};

const schedulerService = new SchedulerService();

export const SchedulerCalendar = observer(function SchedulerCalendar({ workspaceSlug, projectId }: Props) {
  const { t } = useTranslation();

  const [view, setView] = useState<CalendarView>("month");
  const [viewDate, setViewDate] = useState<Date>(() => new Date());
  const [selectedOccurrence, setSelectedOccurrence] = useState<ISchedulerOccurrence | null>(null);
  const [editingBinding, setEditingBinding] = useState<ISchedulerBinding | null>(null);

  const { fromIso, toIso } = useMemo(() => windowForView(view, viewDate), [view, viewDate]);

  const { occurrences, hasMore, isLoading, mutate } = useSchedulerOccurrences({
    workspaceSlug,
    projectId,
    from: fromIso,
    to: toIso,
  });

  // Bindings (for the edit-from-drawer affordance) and workspace schedulers
  // (for the calendars rail) come from the existing endpoints.
  const { data: bindings } = useSWR<ISchedulerBinding[]>(["scheduler-bindings", workspaceSlug, projectId], () =>
    schedulerService.listBindings(workspaceSlug, projectId)
  );
  const { data: workspaceSchedulers } = useSWR<IScheduler[]>(["schedulers", workspaceSlug], () =>
    schedulerService.listSchedulers(workspaceSlug)
  );

  // The rail lists only schedulers that have a binding on this project.
  const installedSchedulers = useMemo(() => {
    if (!workspaceSchedulers || !bindings) return [];
    const bound = new Set(bindings.map((b) => b.scheduler));
    return workspaceSchedulers.filter((s) => bound.has(s.id));
  }, [workspaceSchedulers, bindings]);

  const { isVisible, toggle, showAll, hideAll } = useVisibleSchedulers(
    projectId,
    useMemo(() => installedSchedulers.map((s) => s.id), [installedSchedulers])
  );

  // Filter occurrences by the rail's visibility state.
  const visibleOccurrences = useMemo(
    () => occurrences.filter((o) => isVisible(o.scheduler_id)),
    [occurrences, isVisible]
  );

  // For drawer "edit binding" affordance — match the clicked occurrence to
  // its binding row so the EditSchedulerBindingModal can populate.
  const bindingForOccurrence = useMemo(() => {
    if (!selectedOccurrence || !bindings) return null;
    return bindings.find((b) => b.id === selectedOccurrence.binding_id) ?? null;
  }, [selectedOccurrence, bindings]);

  const empty = !isLoading && installedSchedulers.length === 0;

  return (
    <div className="flex h-full w-full">
      <div className="flex flex-1 flex-col">
        <SchedulerCalendarHeader view={view} viewDate={viewDate} onChangeView={setView} onChangeDate={setViewDate} />

        {empty ? (
          <EmptyState />
        ) : (
          <div className="relative flex-1 overflow-hidden">
            {isLoading && (
              <div className="absolute inset-0 z-20 flex items-center justify-center bg-surface-1/60">
                <Spinner />
              </div>
            )}

            {view === "month" ? (
              <SchedulerMonthView
                viewDate={viewDate}
                occurrences={visibleOccurrences}
                onSelectOccurrence={setSelectedOccurrence}
              />
            ) : (
              <SchedulerWeekView
                viewDate={viewDate}
                occurrences={visibleOccurrences}
                onSelectOccurrence={setSelectedOccurrence}
              />
            )}

            {hasMore && (
              <div className="shadow-md absolute bottom-2 left-1/2 -translate-x-1/2 rounded-md border border-subtle bg-surface-1 px-3 py-1 text-12 text-secondary">
                {t("scheduler_bindings.calendar.too_many")}
              </div>
            )}
          </div>
        )}
      </div>

      <CalendarsRail
        schedulers={installedSchedulers}
        isVisible={isVisible}
        onToggle={toggle}
        onShowAll={showAll}
        onHideAll={hideAll}
      />

      <OccurrenceDrawer
        occurrence={selectedOccurrence}
        binding={bindingForOccurrence}
        onClose={() => setSelectedOccurrence(null)}
        onEditBinding={(b) => {
          setEditingBinding(b);
          setSelectedOccurrence(null);
        }}
      />

      <EditSchedulerBindingModal
        isOpen={!!editingBinding}
        onClose={() => setEditingBinding(null)}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={editingBinding}
        onUpdated={() => {
          mutate();
          setEditingBinding(null);
        }}
      />
    </div>
  );
});

function EmptyState() {
  const { t } = useTranslation();
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-2 p-12 text-center">
      <h3 className="text-16 font-semibold text-primary">{t("scheduler_bindings.calendar.empty_title")}</h3>
      <p className="max-w-md text-13 text-secondary">{t("scheduler_bindings.calendar.empty_body")}</p>
    </div>
  );
}
