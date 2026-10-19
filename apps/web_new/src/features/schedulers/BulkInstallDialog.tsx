// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Catalog-side bulk install (parity/specs/agents.md §2, AGT-006): one
// definition onto many projects with one schedule. The picker lists the
// projects the caller has joined, searches by name or identifier, offers
// select-all, and locks targets that already carry the definition. One
// write per target runs in parallel; the outcome partitions into a success
// and a failure notice, failures stay selected for a retry and successes
// lock as installed.

import { createSchedulerBinding, listSchedulerBindings, type ProjectDetails, type Scheduler } from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../core/api/client.js";
import { projectDetailsQueryOptions } from "../projects/index.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import {
  bulkFailureTitle,
  bulkSelectionSummary,
  bulkSuccessTitle,
  filterProjects,
  failureSummary,
  partitionInstallOutcomes,
  toggleAllBulkTargets,
  toggleBulkTarget,
  type BulkSelection,
} from "./installResults.js";
import { invalidateInstalls } from "./installData.js";
import { emptySchedule, schedulePayload } from "./schedule.js";
import { ScheduleFields, useScheduleDraft } from "./ScheduleFields.js";

export interface BulkInstallDialogProps {
  workspaceSlug: string;
  /** The definition being installed; null keeps the dialog closed. */
  scheduler: Scheduler | null;
  onClose: () => void;
}

/** The projects a workspace member may target for an install. */
export function joinedProjects(projects: readonly ProjectDetails[]): ProjectDetails[] {
  return projects.filter((project) => project.member_role !== null && project.archived_at === null);
}

/** The project's display name, or its raw id when the row is unknown. */
export function projectName(projects: readonly ProjectDetails[], projectId: string): string {
  const match = projects.find((project) => project.id === projectId);
  return match === undefined ? projectId : match.name;
}

function listboxBody(
  loading: boolean,
  visible: readonly ProjectDetails[],
  selection: BulkSelection,
  onChange: (next: BulkSelection) => void
): React.ReactNode {
  if (loading) {
    return <li className="text-body px-(--space-3) py-(--space-2) text-(--text-muted)">Checking installs…</li>;
  }
  if (visible.length === 0) {
    return (
      <li className="text-body px-(--space-3) py-(--space-2) text-(--text-muted)">No projects match your search.</li>
    );
  }
  return visible.map((project) => {
    const locked = selection.installed.includes(project.id);
    const checked = locked || selection.picked.includes(project.id);
    return (
      <li key={project.id}>
        <label className="text-body flex cursor-pointer items-center gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text) hover:bg-(--subtle)">
          <input
            type="checkbox"
            checked={checked}
            disabled={locked}
            onChange={() => onChange(toggleBulkTarget(selection, project.id))}
          />
          <span className="min-w-0 flex-1">
            <span className="block truncate">{project.name}</span>
            <span className="text-meta block text-(--text-muted)">{project.identifier}</span>
            {locked ? <span className="text-meta block text-(--text-muted)">Installed</span> : null}
          </span>
        </label>
      </li>
    );
  });
}

interface PickerProps {
  projects: readonly ProjectDetails[];
  selection: BulkSelection;
  loading: boolean;
  onChange: (next: BulkSelection) => void;
}

function BulkProjectPicker({ projects, selection, loading, onChange }: PickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const rootRef = React.useRef<HTMLDivElement>(null);
  const visible = React.useMemo(() => filterProjects(projects, query), [projects, query]);
  const visibleIds = visible.map((project) => project.id);
  const visibleEligible = visibleIds.filter((id) => !selection.installed.includes(id));
  const allPicked = visibleEligible.length > 0 && visibleEligible.every((id) => selection.picked.includes(id));

  React.useEffect(() => {
    if (!open) return;
    function onPointer(event: MouseEvent): void {
      if (rootRef.current !== null && !rootRef.current.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onPointer);
    return () => document.removeEventListener("mousedown", onPointer);
  }, [open]);

  return (
    <div ref={rootRef} className="relative flex flex-col gap-(--space-2)">
      <span className="text-body font-medium text-(--text)">Projects</span>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        className="text-body flex h-(--control-height) w-full items-center justify-between rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text)"
        onClick={() => setOpen((current) => !current)}
      >
        <span>{bulkSelectionSummary(selection.picked.length)}</span>
      </button>
      {open ? (
        <div
          data-prevent-outside-click=""
          className="shadow-popover absolute top-full left-0 z-10 mt-(--space-1) flex max-h-80 w-full flex-col gap-(--space-2) overflow-hidden rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-3)"
        >
          <Input
            label="Search projects"
            placeholder="Search projects…"
            aria-label="Search projects"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <div className="flex justify-end">
            <Button
              size="small"
              variant="ghost"
              disabled={loading}
              onClick={() => onChange(toggleAllBulkTargets(selection, visibleIds))}
            >
              {allPicked ? "Clear selection" : "Select all"}
            </Button>
          </div>
          <ul role="listbox" aria-label="Projects" aria-multiselectable="true" className="overflow-y-auto">
            {listboxBody(loading, visible, selection, onChange)}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

function BulkInstallForm({
  workspaceSlug,
  scheduler,
  onClose,
}: {
  workspaceSlug: string;
  scheduler: Scheduler;
  onClose: () => void;
}): React.ReactElement {
  const queryClient = useQueryClient();
  const projects = useQuery(projectDetailsQueryOptions(getAppClient(), workspaceSlug));
  const [selection, setSelection] = React.useState<BulkSelection>({ picked: [], installed: [] });
  const [detecting, setDetecting] = React.useState(true);
  const [submitting, setSubmitting] = React.useState(false);
  const schedule = useScheduleDraft(emptySchedule());

  const joined = React.useMemo(() => joinedProjects(projects.data ?? []), [projects.data]);

  React.useEffect(() => {
    if (!projects.isSuccess) return;
    let cancelled = false;
    setDetecting(true);
    const targets = joinedProjects(projects.data);
    void (async () => {
      const checks = await Promise.all(
        targets.map(async (project) => {
          try {
            const bindings = await listSchedulerBindings(getAppClient(), workspaceSlug, project.id);
            return bindings.some((binding) => binding.scheduler === scheduler.id) ? project.id : null;
          } catch {
            // A project whose installs cannot be read stays offerable; the
            // write's own refusal is what the user sees.
            return null;
          }
        })
      );
      if (cancelled) return;
      setSelection({ picked: [], installed: checks.filter((id): id is string => id !== null) });
      setDetecting(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [joined, projects.isSuccess, projects.data, scheduler.id, workspaceSlug]);

  async function submit(): Promise<void> {
    if (submitting) return;
    const targets = selection.picked.filter((id) => !selection.installed.includes(id));
    if (targets.length === 0) {
      notifyError("Select a project", "Pick at least one project to install this scheduler on.");
      return;
    }
    if (!schedule.validate()) return;
    const payload = schedulePayload(schedule.value);
    setSubmitting(true);
    const outcomes = await Promise.allSettled(
      targets.map((projectId) =>
        createSchedulerBinding(getAppClient(), workspaceSlug, projectId, {
          scheduler: scheduler.id,
          ...payload,
        })
      )
    );
    setSubmitting(false);
    const partition = partitionInstallOutcomes(targets, outcomes);
    if (partition.succeeded.length > 0) {
      notifySuccess(bulkSuccessTitle(partition.succeeded.length), "It will fire on the configured schedule.");
      await Promise.all(
        [...new Set(partition.succeeded)].map((id) => invalidateInstalls(queryClient, workspaceSlug, id))
      );
    }
    if (partition.failed.length > 0) {
      const names = partition.failed.map((failure) => projectName(joined, failure.targetId)).join(", ");
      notifyError(bulkFailureTitle(partition.failed.length), `${names} — ${failureSummary(partition)}`);
      const failedIds = partition.failed.map((failure) => failure.targetId);
      setSelection((current) => ({
        picked: failedIds,
        installed: [...current.installed, ...partition.succeeded.filter((id) => !current.installed.includes(id))],
      }));
      return;
    }
    onClose();
  }

  return (
    <form
      aria-label={`Install ${scheduler.name}`}
      className="flex flex-col gap-(--space-6)"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <div className="flex flex-col gap-(--space-1)">
        <div className="flex items-center gap-(--space-2)">
          <span
            aria-hidden="true"
            className="inline-block size-2.5 shrink-0 rounded-sm"
            style={{ backgroundColor: scheduler.color }}
          />
          <span className="text-h3 text-(--text)">Install {scheduler.name}</span>
        </div>
        <p className="text-body text-(--text-muted)">{scheduler.description}</p>
      </div>

      {joined.length === 0 ? (
        <p className="text-body rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-6) text-center text-(--text-muted)">
          You haven't joined a project in this workspace to install on.
        </p>
      ) : (
        <BulkProjectPicker projects={joined} selection={selection} loading={detecting} onChange={setSelection} />
      )}

      <ScheduleFields
        idPrefix="bulk-install"
        value={schedule.value}
        errors={schedule.errors}
        disabled={submitting}
        onChange={schedule.change}
      />

      <div className="flex justify-end gap-(--space-3)">
        <Button variant="ghost" disabled={submitting} onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" loading={submitting} disabled={submitting || joined.length === 0}>
          {submitting ? "Installing…" : "Install"}
        </Button>
      </div>
    </form>
  );
}

export function BulkInstallDialog({ workspaceSlug, scheduler, onClose }: BulkInstallDialogProps): React.ReactElement {
  return (
    <Dialog
      open={scheduler !== null}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={scheduler === null ? "Install scheduler" : `Install ${scheduler.name}`}
      description="One schedule applies to every selected project."
      className="max-w-2xl"
    >
      {scheduler === null ? null : (
        <BulkInstallForm key={scheduler.id} workspaceSlug={workspaceSlug} scheduler={scheduler} onClose={onClose} />
      )}
    </Dialog>
  );
}
