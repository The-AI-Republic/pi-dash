// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners list page (ADM-080, from parity/specs/admin.md §11): the
// provisioning filter with its total count, the runners table, and
// the prev/next pager. The backend answers newest-first at fifty
// rows per page; changing the filter restarts from page 1. The old
// app has no runners surface (bug: NEWFRONT-494), so the copy here
// is fresh rather than oracle-pinned.
//
// Parity-driver contract (for the NEWFRONT-521 gate author): the page
// root is `runners-page` with `runners-title` / `runners-description`
// ("Runners" and its one-line summary). The provisioning filter is
// the `runners-filter` select with option values "" (labelled "all"),
// "manual", "desktop_bundled"; the `runners-total` line beside it
// reads "N runners" ("1 runner" for one). The table is
// `runners-table` with headers Name, Workspace, Owner,
// Provisioning, Status, Runner version, Codex version, Last
// heartbeat, and one eight-cell row per runner; absent versions,
// slugs, owners, and heartbeats read "—", and the heartbeat renders
// in locale format. An empty read keeps the headers with the
// `runners-empty` message row ("No runners found."). Loading shows
// `runners-skeleton` (role="status") with two 48px bars; a failed
// read shows `runners-error` with a Retry button. The
// `runners-pager` renders only past one page, with `runners-prev` /
// `runners-next` buttons around the `runners-page-label` ("Page N");
// each side disables at its bound.
import { Button, FieldScaffold, Skeleton } from "@pidash/kit";
import type { InstanceRunner, InstanceRunnerListResponse } from "@pidash/api-client";
import * as React from "react";

import { useInstanceRunners } from "../hooks.js";
import {
  formatProvisioningOptionLabel,
  formatRunnerCellText,
  formatRunnerHeartbeat,
  formatRunnerTotal,
  formatRunnersPageLabel,
  RUNNERS_DESCRIPTION,
  RUNNERS_EMPTY_TEXT,
  RUNNERS_FILTER_LABEL,
  RUNNERS_FILTER_OPTIONS,
  RUNNERS_HEADERS,
  RUNNERS_HEADING,
  RUNNERS_NEXT_LABEL,
  RUNNERS_PREV_LABEL,
  RUNNERS_SKELETON_BAR_HEIGHT,
  RUNNERS_SKELETON_ROWS,
  runnersPageCount,
} from "../runners.js";

function RunnersSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading runners" data-testid="runners-skeleton">
      {Array.from({ length: RUNNERS_SKELETON_ROWS }).map((_, index) => (
        <Skeleton key={index} height={RUNNERS_SKELETON_BAR_HEIGHT} />
      ))}
    </div>
  );
}

function RunnersError({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <div data-testid="runners-error" className="flex flex-col items-start gap-(--space-3)">
      <p className="text-body text-(--text)">The runners failed to load.</p>
      <Button variant="primary" onClick={onRetry}>
        Retry
      </Button>
    </div>
  );
}

function RunnersFilter({
  value,
  total,
  onChange,
}: {
  value: string;
  total: number;
  onChange: (provisioning: string) => void;
}): React.ReactElement {
  const fieldId = React.useId();
  return (
    <div className="flex items-end gap-(--space-4)">
      <FieldScaffold label={RUNNERS_FILTER_LABEL} fieldId={fieldId}>
        {(describedBy) => (
          <select
            id={fieldId}
            aria-describedby={describedBy}
            value={value}
            onChange={(event) => {
              onChange(event.target.value);
            }}
            data-testid="runners-filter"
            className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) focus:border-(--accent) focus:outline-none"
          >
            {RUNNERS_FILTER_OPTIONS.map((provisioning) => (
              <option key={provisioning} value={provisioning}>
                {formatProvisioningOptionLabel(provisioning)}
              </option>
            ))}
          </select>
        )}
      </FieldScaffold>
      <p data-testid="runners-total" className="text-body pb-(--space-2) text-(--text-muted)">
        {formatRunnerTotal(total)}
      </p>
    </div>
  );
}

function RunnerRow({ runner }: { runner: InstanceRunner }): React.ReactElement {
  return (
    <tr>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) font-medium text-(--text)">
        {runner.name}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
        {formatRunnerCellText(runner.workspace_slug)}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
        {formatRunnerCellText(runner.owner_email)}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
        {runner.provisioning}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text)">
        {runner.status}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)">
        {formatRunnerCellText(runner.runner_version)}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)">
        {formatRunnerCellText(runner.codex_version)}
      </td>
      <td className="text-body border-b border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)">
        {formatRunnerHeartbeat(runner.last_heartbeat_at)}
      </td>
    </tr>
  );
}

function RunnersTable({ page }: { page: InstanceRunnerListResponse }): React.ReactElement {
  const rows = page.results;
  return (
    <table data-testid="runners-table" className="w-full border-collapse text-left">
      <thead>
        <tr>
          {RUNNERS_HEADERS.map((header) => (
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
        {rows.map((runner) => (
          <RunnerRow key={runner.id} runner={runner} />
        ))}
        {rows.length === 0 ? (
          <tr>
            <td
              colSpan={RUNNERS_HEADERS.length}
              data-testid="runners-empty"
              className="text-body px-(--space-4) py-(--space-4) text-center text-(--text-muted)"
            >
              {RUNNERS_EMPTY_TEXT}
            </td>
          </tr>
        ) : null}
      </tbody>
    </table>
  );
}

function RunnersPager({
  page,
  pageCount,
  onPage,
}: {
  page: number;
  pageCount: number;
  onPage: (next: number) => void;
}): React.ReactElement | null {
  if (pageCount <= 1) return null;
  return (
    <div data-testid="runners-pager" className="flex items-center gap-(--space-3)">
      <Button
        variant="secondary"
        data-testid="runners-prev"
        disabled={page <= 1}
        onClick={() => {
          onPage(page - 1);
        }}
      >
        {RUNNERS_PREV_LABEL}
      </Button>
      <span data-testid="runners-page-label" className="text-body text-(--text-muted)">
        {formatRunnersPageLabel(page)}
      </span>
      <Button
        variant="secondary"
        data-testid="runners-next"
        disabled={page >= pageCount}
        onClick={() => {
          onPage(page + 1);
        }}
      >
        {RUNNERS_NEXT_LABEL}
      </Button>
    </div>
  );
}

export function AdminRunnersPage(): React.ReactElement {
  const [provisioning, setProvisioning] = React.useState("");
  const [page, setPage] = React.useState(1);
  const runners = useInstanceRunners(provisioning, page);
  let body: React.ReactElement;
  if (runners.isPending) {
    body = <RunnersSkeleton />;
  } else if (runners.isError) {
    body = (
      <RunnersError
        onRetry={() => {
          void runners.refetch();
        }}
      />
    );
  } else {
    const response: InstanceRunnerListResponse = runners.data;
    body = (
      <div className="flex flex-col gap-(--space-4)">
        <RunnersFilter
          value={provisioning}
          total={response.total}
          onChange={(next) => {
            setProvisioning(next);
            setPage(1);
          }}
        />
        <RunnersTable page={response} />
        <RunnersPager
          page={response.page}
          pageCount={runnersPageCount(response.total)}
          onPage={(next) => {
            setPage(next);
          }}
        />
      </div>
    );
  }
  return (
    <div data-testid="runners-page" className="w-full p-(--space-6)">
      <div className="mb-(--space-4)">
        <h1 data-testid="runners-title" className="text-h2 text-(--text)">
          {RUNNERS_HEADING}
        </h1>
        <p data-testid="runners-description" className="text-body text-(--text-muted)">
          {RUNNERS_DESCRIPTION}
        </p>
      </div>
      {body}
    </div>
  );
}
