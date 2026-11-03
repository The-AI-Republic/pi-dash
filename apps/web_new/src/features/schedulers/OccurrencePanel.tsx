// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Occurrence side panel (parity/specs/agents.md §5, AGT-017). One firing
// explained in place: when it fires in the install's zone, that zone, how
// often it repeats, the project context the install carries, a link to
// the parent install, and (project admins only) the shared edit dialog.
// It renders nothing until a block is picked and never covers the edit
// dialog: choosing Edit closes it first.

import type { SchedulerBinding, SchedulerOccurrence } from "@pidash/api-client";
import { Button, IconButton } from "@pidash/kit";
import { X } from "lucide-react";
import * as React from "react";

import {
  formatOccurrenceInstant,
  occurrenceContext,
  occurrenceRecurrence,
  occurrenceRunHref,
  occurrenceStatus,
} from "./calendarText.js";

export interface OccurrencePanelProps {
  occurrence: SchedulerOccurrence;
  /** The parent install, when the project's install list has it. */
  binding: SchedulerBinding | null;
  /** Install detail address the panel links to. */
  installHref: string;
  /** Project admins get the edit entry point; everyone else does not. */
  canEdit: boolean;
  onClose: () => void;
  onEdit: (binding: SchedulerBinding) => void;
  workspaceSlug: string;
  projectId: string;
}

function DetailRow({ label, children }: { label: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div data-testid="occurrence-detail" className="mb-3 grid grid-cols-[7rem_minmax(0,1fr)] gap-(--space-3)">
      <div className="text-meta font-medium tracking-wide text-(--text-muted) uppercase">{label}</div>
      <div className="text-body text-(--text)">{children}</div>
    </div>
  );
}

export function OccurrencePanel({
  occurrence,
  binding,
  installHref,
  canEdit,
  onClose,
  onEdit,
  workspaceSlug,
  projectId,
}: OccurrencePanelProps): React.ReactElement {
  const runHref = occurrenceRunHref(workspaceSlug, projectId, occurrence);
  const status = occurrenceStatus(occurrence);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`${occurrence.scheduler_name} occurrence`}
      className="shadow-lg fixed inset-y-0 right-0 z-40 flex w-[min(26rem,92vw)] flex-col border-l border-(--border) bg-(--surface)"
    >
      <header className="flex items-start justify-between gap-(--space-3) border-b border-(--border) p-(--space-5)">
        <div className="flex flex-col gap-(--space-1)">
          <div
            data-testid="occurrence-status"
            className="text-meta font-medium tracking-wide text-(--text-muted) uppercase"
          >
            {status}
          </div>
          <h2 className="text-h4-medium text-(--text)">{occurrence.scheduler_name}</h2>
        </div>
        <IconButton label="Close" variant="ghost" onClick={onClose}>
          <X size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto p-(--space-5)">
        <DetailRow label="When">{formatOccurrenceInstant(occurrence.dtstart, occurrence.tzid)}</DetailRow>
        <DetailRow label="Time zone">{occurrence.tzid}</DetailRow>
        <DetailRow label="Recurrence">{occurrenceRecurrence(binding)}</DetailRow>
        <DetailRow label="Project context">{occurrenceContext(binding)}</DetailRow>
        {runHref === null ? null : (
          <div className="mb-3">
            <a className="text-body font-medium text-(--accent) hover:underline" href={runHref}>
              View run →
            </a>
          </div>
        )}
        <div className="mt-(--space-5) border-t border-(--border) pt-(--space-4)">
          <a className="text-body font-medium text-(--accent) hover:underline" href={installHref}>
            View scheduler →
          </a>
        </div>
      </div>
      <footer className="flex items-center justify-end gap-(--space-3) border-t border-(--border) p-(--space-4)">
        <Button variant="ghost" onClick={onClose}>
          Close
        </Button>
        {canEdit && binding !== null ? (
          <Button variant="primary" onClick={() => onEdit(binding)}>
            Edit binding
          </Button>
        ) : null}
      </footer>
    </div>
  );
}
