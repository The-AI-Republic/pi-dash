// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job detail page (ADM-058/059/060/061/073, from
// parity/specs/admin.md §7.8–7.11): the job header with edit and
// delete actions, the 24h stat tiles, the skip-reason filter, and the
// targets table. Delete runs immediately with no confirmation — the
// load-bearing absence, not a gap to fix. Strings marked
// oracle-pinned are asserted verbatim by
// e2e/parity/admin/loop.spec.ts.
//
// Parity-driver contract (for the NEWFRONT-521 gate author): the page
// root is `loop-job-detail-page`. The header title and description
// are `loop-job-detail-title` / `loop-job-detail-description` (the
// title reads "Loop job" while loading); Edit/Delete are exact-name
// buttons (`loop-job-detail-edit` / `loop-job-detail-delete`),
// hidden while loading. Stat tiles live under
// `loop-job-detail-stats` as `loop-job-detail-stat-targets`,
// `-completed`, `-failed`, `-skipped`, each carrying its numeric
// value and its label ("Targets", "Completed (24h)", "Failed (24h)",
// "Skipped (24h)"); the block hides when the job carries no stats.
// The skip filter is the `loop-job-detail-skip-filter` select with
// option values "" then the seven skip reasons. The targets table is
// `loop-job-detail-targets` with the pinned headers and one six-cell
// row per target; the empty read shows `loop-job-detail-targets-empty`
// ("No targets yet."), and missing cells read "—". Loading shows
// `loop-job-detail-skeleton` (role="status") with 48px/120px bars.
// Delete fires DELETE at once with no dialog: success toasts
// "Deleted" / "Loop job deleted." and lands on "/god-mode/loop/",
// failure toasts "Delete failed" / "Could not delete the job." and
// stays.
import { useNavigate } from "@tanstack/react-router";
import { Button, FieldScaffold, Skeleton } from "@pidash/kit";
import type { LoopJobDetail, LoopJobStats, LoopTargetsPage } from "@pidash/api-client";
import * as React from "react";

import { notifySuccess } from "../../../shared/shell/index.js";
import { useDeleteLoopJob, useLoopJobDetail, useLoopJobTargets } from "../hooks.js";
import {
  formatSkipOptionLabel,
  formatTargetLastRunStatus,
  formatTargetNextRun,
  formatTargetSkipReason,
  formatTargetTokens,
  LOOP_JOB_DELETED_MESSAGE,
  LOOP_JOB_DELETED_TITLE,
  LOOP_JOB_DETAIL_SKELETON_HEIGHTS,
  LOOP_JOB_DETAIL_SKELETON_TITLE,
  LOOP_JOB_STAT_LABELS,
  LOOP_JOBS_LIST_PATH,
  LOOP_SKIP_FILTER_LABEL,
  LOOP_SKIP_FILTER_OPTIONS,
  LOOP_TARGETS_EMPTY_TEXT,
  LOOP_TARGETS_HEADERS,
  resolveLoopJobDescription,
} from "../loopJobDetail.js";
import { AdminLoopJobModal } from "./AdminLoopJobModal.js";

function LoopJobDetailSkeleton(): React.ReactElement {
  return (
    <div data-testid="loop-job-detail-page" className="w-full p-(--space-6)">
      <h1 data-testid="loop-job-detail-title" className="text-h2 mb-(--space-4) text-(--text)">
        {LOOP_JOB_DETAIL_SKELETON_TITLE}
      </h1>
      <div role="status" aria-label="Loading loop job" data-testid="loop-job-detail-skeleton">
        {LOOP_JOB_DETAIL_SKELETON_HEIGHTS.map((height) => (
          <Skeleton key={height} height={height} />
        ))}
      </div>
    </div>
  );
}

function LoopJobDetailError({
  testId,
  message,
  onRetry,
}: {
  testId: string;
  message: string;
  onRetry: () => void;
}): React.ReactElement {
  return (
    <div data-testid={testId} className="flex flex-col items-start gap-(--space-3)">
      <p className="text-body text-(--text)">{message}</p>
      <Button variant="primary" onClick={onRetry}>
        Retry
      </Button>
    </div>
  );
}

function LoopJobStatTiles({ stats }: { stats: LoopJobStats | null | undefined }): React.ReactElement | null {
  if (!stats) return null;
  const tiles = [
    { testId: "loop-job-detail-stat-targets", label: LOOP_JOB_STAT_LABELS.targets, value: stats.target_count },
    { testId: "loop-job-detail-stat-completed", label: LOOP_JOB_STAT_LABELS.completed, value: stats.completed },
    { testId: "loop-job-detail-stat-failed", label: LOOP_JOB_STAT_LABELS.failed, value: stats.failed },
    { testId: "loop-job-detail-stat-skipped", label: LOOP_JOB_STAT_LABELS.skipped, value: stats.skipped },
  ] as const;
  return (
    <div data-testid="loop-job-detail-stats" className="flex gap-(--space-3)">
      {tiles.map((tile) => (
        <div
          key={tile.testId}
          data-testid={tile.testId}
          className="flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3)"
        >
          <div className="text-h3 text-(--text)">{tile.value}</div>
          <div className="text-meta text-(--text-muted)">{tile.label}</div>
        </div>
      ))}
    </div>
  );
}

function LoopSkipFilter({
  value,
  onChange,
}: {
  value: string;
  onChange: (reason: string) => void;
}): React.ReactElement {
  const fieldId = React.useId();
  return (
    <FieldScaffold label={LOOP_SKIP_FILTER_LABEL} fieldId={fieldId}>
      {(describedBy) => (
        <select
          id={fieldId}
          aria-describedby={describedBy}
          value={value}
          onChange={(event) => {
            onChange(event.target.value);
          }}
          data-testid="loop-job-detail-skip-filter"
          className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) focus:border-(--accent) focus:outline-none"
        >
          {LOOP_SKIP_FILTER_OPTIONS.map((reason) => (
            <option key={reason} value={reason}>
              {formatSkipOptionLabel(reason)}
            </option>
          ))}
        </select>
      )}
    </FieldScaffold>
  );
}

function LoopTargetsTable({ page }: { page: LoopTargetsPage | undefined }): React.ReactElement {
  const rows = page?.results ?? [];
  return (
    <table data-testid="loop-job-detail-targets" className="w-full border-collapse text-left">
      <thead>
        <tr>
          {LOOP_TARGETS_HEADERS.map((header) => (
            <th
              key={header}
              scope="col"
              className="text-meta border-b border-(--border) px-(--space-4) py-(--space-2) font-medium text-(--text-muted)"
            >
              {header}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((target) => (
          <tr key={target.id}>
            <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
              {target.workspace_slug}
            </td>
            <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
              {target.user_email}
            </td>
            <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)">
              {formatTargetNextRun(target.next_run_at)}
            </td>
            <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
              {formatTargetLastRunStatus(target.last_run)}
            </td>
            <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)">
              {formatTargetTokens(target.last_run)}
            </td>
            <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)">
              {formatTargetSkipReason(target.last_skip_reason)}
            </td>
          </tr>
        ))}
        {page !== undefined && rows.length === 0 ? (
          <tr>
            <td
              colSpan={LOOP_TARGETS_HEADERS.length}
              data-testid="loop-job-detail-targets-empty"
              className="text-body px-(--space-4) py-(--space-4) text-center text-(--text-muted)"
            >
              {LOOP_TARGETS_EMPTY_TEXT}
            </td>
          </tr>
        ) : null}
      </tbody>
    </table>
  );
}

export function AdminLoopJobDetailPage({ jobId }: { jobId: string }): React.ReactElement {
  const job = useLoopJobDetail(jobId);
  const [skipFilter, setSkipFilter] = React.useState("");
  const targets = useLoopJobTargets(jobId, skipFilter);
  const [editing, setEditing] = React.useState(false);
  const deletion = useDeleteLoopJob();
  const navigate = useNavigate();

  if (job.isPending) {
    return <LoopJobDetailSkeleton />;
  }
  if (job.isError) {
    return (
      <div data-testid="loop-job-detail-page" className="w-full p-(--space-6)">
        <LoopJobDetailError
          testId="loop-job-detail-error"
          message="The loop job failed to load."
          onRetry={() => {
            void job.refetch();
          }}
        />
      </div>
    );
  }
  const detail: LoopJobDetail = job.data;
  const remove = (): void => {
    deletion.mutate(jobId, {
      onSuccess: () => {
        notifySuccess(LOOP_JOB_DELETED_TITLE, LOOP_JOB_DELETED_MESSAGE);
        void navigate({ to: LOOP_JOBS_LIST_PATH });
      },
    });
  };
  return (
    <div data-testid="loop-job-detail-page" className="w-full p-(--space-6)">
      <div className="mb-(--space-4) flex items-start justify-between gap-(--space-4)">
        <div>
          <h1 data-testid="loop-job-detail-title" className="text-h2 text-(--text)">
            {detail.name}
          </h1>
          <p data-testid="loop-job-detail-description" className="text-body text-(--text-muted)">
            {resolveLoopJobDescription(detail.public_description)}
          </p>
        </div>
        <div className="flex gap-(--space-2)">
          <Button
            variant="secondary"
            onClick={() => {
              setEditing(true);
            }}
            data-testid="loop-job-detail-edit"
          >
            Edit
          </Button>
          <Button variant="danger" onClick={remove} disabled={deletion.isPending} data-testid="loop-job-detail-delete">
            Delete
          </Button>
        </div>
      </div>
      <div className="flex flex-col gap-(--space-4)">
        <LoopJobStatTiles stats={detail.stats} />
        <LoopSkipFilter
          value={skipFilter}
          onChange={(reason) => {
            setSkipFilter(reason);
          }}
        />
        {targets.isError ? (
          <LoopJobDetailError
            testId="loop-job-detail-targets-error"
            message="The targets failed to load."
            onRetry={() => {
              void targets.refetch();
            }}
          />
        ) : (
          <LoopTargetsTable page={targets.data} />
        )}
      </div>
      {editing ? (
        <AdminLoopJobModal
          job={detail}
          onClose={() => {
            setEditing(false);
          }}
        />
      ) : null}
    </div>
  );
}
