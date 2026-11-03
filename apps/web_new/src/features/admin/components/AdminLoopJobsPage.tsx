// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop jobs list page (ADM-051–055, from parity/specs/admin.md
// §7.1–7.5): header with the New-job action, jobs table with inline
// enabled toggles, the inline empty paragraph, and the loading
// skeleton. The detail slice (NEWFRONT-517) owns
// /god-mode/loop/<id>/, which the name links below already target.
// Strings marked oracle-pinned are asserted verbatim by
// e2e/parity/admin/loop.spec.ts.
import { Link } from "@tanstack/react-router";
import { Button, Skeleton, Switch } from "@pidash/kit";
import type { LoopJobList } from "@pidash/api-client";
import * as React from "react";

import { useLoopJobs, useToggleLoopJob, type LoopJobToggle } from "../hooks.js";
import { LOOP_JOB_MODAL_CREATE_TITLE } from "../loopJobModal.js";
import { LOOP_LIST_EMPTY_TEXT, resolveLoopMinRoleLabel } from "../loopJobs.js";
import { AdminLoopJobModal } from "./AdminLoopJobModal.js";

/** Page heading; the suite pins its presence, not its wording. */
export const LOOP_JOBS_HEADING = "Loop jobs";

/** Page description; the suite pins its presence, not its wording. */
export const LOOP_JOBS_DESCRIPTION = "Automatic jobs that run on a schedule across workspaces.";

/**
 * Table column headers, left to right (oracle-pinned:
 * loop.spec.ts ADM-051).
 */
export const LOOP_LIST_HEADERS: readonly string[] = ["Name", "Slug", "Recurrence", "Min role", "Builtin", "Enabled"];

/** Skeleton bar count and height (oracle-pinned: loop.spec.ts ADM-054). */
export const LOOP_LIST_SKELETON_ROWS = 2;
export const LOOP_LIST_SKELETON_BAR_HEIGHT = "48px";

/** Builtin cell text (oracle-pinned behavior: loop.spec.ts ADM-051 reads "Yes"). */
export const LOOP_BUILTIN_YES = "Yes";
export const LOOP_BUILTIN_NO = "No";

function LoopJobRow({
  job,
  onToggle,
}: {
  job: LoopJobList[number];
  onToggle: (toggle: LoopJobToggle) => void;
}): React.ReactElement {
  return (
    <tr>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) font-medium text-(--text)">
        <Link
          to="/god-mode/loop/$jobId"
          params={{ jobId: job.id }}
          className="text-(--accent) hover:text-(--accent-hover) hover:underline"
        >
          {job.name}
        </Link>
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">{job.slug}</td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) break-all text-(--text)">
        {job.rrule}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
        {resolveLoopMinRoleLabel(job.min_role)}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
        {job.is_builtin ? LOOP_BUILTIN_YES : LOOP_BUILTIN_NO}
      </td>
      <td className="border-b border-(--border) px-(--space-4) py-(--space-3)">
        <Switch
          checked={job.enabled}
          label={`${job.name} enabled`}
          onCheckedChange={(enabled) => {
            onToggle({ id: job.id, enabled });
          }}
        />
      </td>
    </tr>
  );
}

function LoopJobsTable({
  jobs,
  onToggle,
}: {
  jobs: LoopJobList;
  onToggle: (toggle: LoopJobToggle) => void;
}): React.ReactElement {
  return (
    <table data-testid="loop-jobs-table" className="w-full border-collapse text-left">
      <thead>
        <tr>
          {LOOP_LIST_HEADERS.map((header) => (
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
        {jobs.map((job) => (
          <LoopJobRow key={job.id} job={job} onToggle={onToggle} />
        ))}
      </tbody>
    </table>
  );
}

function LoopJobsSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading loop jobs" data-testid="loop-jobs-skeleton">
      {Array.from({ length: LOOP_LIST_SKELETON_ROWS }).map((_, index) => (
        <Skeleton key={index} height={LOOP_LIST_SKELETON_BAR_HEIGHT} />
      ))}
    </div>
  );
}

function LoopJobsError({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <div data-testid="loop-jobs-error" className="flex flex-col items-start gap-(--space-3)">
      <p className="text-body text-(--text)">The loop jobs failed to load.</p>
      <Button variant="primary" onClick={onRetry}>
        Retry
      </Button>
    </div>
  );
}

export function AdminLoopJobsPage(): React.ReactElement {
  const jobs = useLoopJobs();
  const toggle = useToggleLoopJob();
  const [createOpen, setCreateOpen] = React.useState(false);
  let body: React.ReactElement;
  if (jobs.isPending) {
    body = <LoopJobsSkeleton />;
  } else if (jobs.isError) {
    body = (
      <LoopJobsError
        onRetry={() => {
          void jobs.refetch();
        }}
      />
    );
  } else if (jobs.data.length === 0) {
    body = (
      <p data-testid="loop-jobs-empty" className="text-body text-(--text-muted)">
        {LOOP_LIST_EMPTY_TEXT}
      </p>
    );
  } else {
    body = (
      <LoopJobsTable
        jobs={jobs.data}
        onToggle={(input) => {
          toggle.mutate(input);
        }}
      />
    );
  }
  return (
    <div data-testid="loop-jobs-page" className="w-full p-(--space-6)">
      <div className="mb-(--space-4) flex items-start justify-between gap-(--space-4)">
        <div>
          <h1 className="text-h2 text-(--text)">{LOOP_JOBS_HEADING}</h1>
          <p className="text-body text-(--text-muted)">{LOOP_JOBS_DESCRIPTION}</p>
        </div>
        <Button
          variant="primary"
          onClick={() => {
            setCreateOpen(true);
          }}
          data-testid="loop-jobs-create"
        >
          {LOOP_JOB_MODAL_CREATE_TITLE}
        </Button>
      </div>
      {body}
      {createOpen ? (
        <AdminLoopJobModal
          onClose={() => {
            setCreateOpen(false);
          }}
        />
      ) : null}
    </div>
  );
}
