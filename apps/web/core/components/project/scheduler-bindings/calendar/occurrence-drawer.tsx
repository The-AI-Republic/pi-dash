/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useMemo } from "react";
import { X as CloseIcon } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { Button } from "@pi-dash/propel/button";
import type { ISchedulerBinding, ISchedulerOccurrence } from "@pi-dash/services";
import { DEFAULT_SCHEDULER_COLOR } from "../constants";
import { humanizeRrule } from "../rrule-text";

type Props = {
  occurrence: ISchedulerOccurrence | null;
  /** Closest matching binding (used to populate the edit-from-drawer affordance). */
  binding: ISchedulerBinding | null;
  /** Whether the viewing user can mutate bindings on this project. */
  canManage: boolean;
  onClose: () => void;
  onEditBinding: (binding: ISchedulerBinding) => void;
};

/**
 * Side-drawer surfaced on occurrence click. Past occurrences show the
 * AgentRun summary + a link to the full run; future occurrences show the
 * scheduler/RRULE + an "edit binding" button.
 *
 * Wires straight off ``ISchedulerOccurrence`` — no extra fetch for the
 * happy path. The full AgentRun detail (events, output, etc.) opens in
 * a separate page or panel reused from the runner module.
 */
export function OccurrenceDrawer({ occurrence, binding, canManage, onClose, onEditBinding }: Props) {
  const { t } = useTranslation();

  const friendlyRule = useMemo(() => (binding ? humanizeRrule(binding.rrule, binding.dtstart) : null), [binding]);

  if (!occurrence) return null;

  const dt = new Date(occurrence.dtstart);
  const isPast = occurrence.kind === "past";

  return (
    <div className="shadow-xl fixed inset-y-0 right-0 z-40 flex w-[420px] max-w-full flex-col border-l border-subtle bg-surface-1 md:w-[420px]">
      <header className="flex items-center justify-between border-b border-subtle px-4 py-3">
        <div className="flex items-center gap-2">
          <span
            className="inline-block size-4 rounded-sm"
            style={{ backgroundColor: occurrence.scheduler_color || DEFAULT_SCHEDULER_COLOR }}
          />
          <h2 className="text-14 font-medium text-primary">{occurrence.scheduler_name}</h2>
        </div>
        <button type="button" onClick={onClose} className="text-secondary hover:text-primary" aria-label="Close">
          <CloseIcon className="size-4" />
        </button>
      </header>

      <div className="flex-1 overflow-y-auto p-4">
        <div className="mb-4 text-12 tracking-wide text-tertiary uppercase">
          {isPast
            ? t("Past run")
            : t("Scheduled")}
        </div>

        <Row label={t("When")} value={dt.toLocaleString()} />
        <Row label={t("Time zone")} value={occurrence.tzid} />
        {isPast && occurrence.status && (
          <Row label={t("Status")} value={occurrence.status} />
        )}

        {!isPast && binding && (
          <>
            {friendlyRule && <Row label={t("Recurrence")} value={friendlyRule} />}
            {binding.extra_context && (
              <Row label={t("Project context")} value={binding.extra_context} multiline />
            )}
          </>
        )}

        {isPast && occurrence.agent_run_id && (
          <div className="mt-6">
            <Button variant="link" size="sm" onClick={onClose}>
              {t("View full run")} →
            </Button>
          </div>
        )}
      </div>

      {!isPast && binding && canManage && (
        <footer className="flex justify-end gap-2 border-t border-subtle px-4 py-3">
          <Button variant="secondary" size="sm" onClick={onClose}>
            {t("Close")}
          </Button>
          <Button variant="primary" size="sm" onClick={() => onEditBinding(binding)}>
            {t("Edit binding")}
          </Button>
        </footer>
      )}
    </div>
  );
}

function Row({ label, value, multiline = false }: { label: string; value: string; multiline?: boolean }) {
  return (
    <div className="mb-3">
      <div className="text-12 tracking-wide text-tertiary uppercase">{label}</div>
      <div className={multiline ? "mt-1 text-13 whitespace-pre-wrap text-primary" : "mt-1 text-13 text-primary"}>
        {value}
      </div>
    </div>
  );
}
