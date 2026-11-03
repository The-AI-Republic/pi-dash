// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Previous-exports history section (parity/specs/exports.md §3,
// EXP-006–013). The settings-page slice mounts this below the export
// form: rows with states and download cells, live re-reads while work
// runs, cursor paging, and skeleton/empty/error treatments.
import * as React from "react";
import { Avatar, Badge, Button, EmptyState, Skeleton, cn } from "@pidash/kit";
import type { WorkspaceExport } from "@pidash/api-client";

import { useExportsHistoryPage } from "../api/history.js";
import {
  EXPORT_HISTORY_DOWNLOAD_IDLE,
  EXPORT_HISTORY_DOWNLOAD_LABEL,
  EXPORT_HISTORY_EMPTY_DESCRIPTION,
  EXPORT_HISTORY_EMPTY_TITLE,
  EXPORT_HISTORY_ERROR_MESSAGE,
  EXPORT_HISTORY_EXPIRED_LABEL,
  EXPORT_HISTORY_FIRST_CURSOR,
  EXPORT_HISTORY_LOADING_LABEL,
  EXPORT_HISTORY_REFRESH_BUSY,
  EXPORT_HISTORY_REFRESH_IDLE,
  EXPORT_HISTORY_TITLE,
  historyRowFacts,
  type ExportHistoryRowFacts,
} from "../exportHistory.js";

export interface ExportsHistoryProps {
  workspaceSlug: string;
}

function DownloadCell({ facts }: { facts: ExportHistoryRowFacts }): React.ReactElement {
  if (facts.download.kind === "link") {
    return (
      <a
        href={facts.download.href}
        target="_blank"
        rel="noreferrer"
        aria-label={`Download export from ${facts.startedLabel}`}
        className={cn(
          "text-(--accent) underline-offset-2 hover:underline",
          "focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid"
        )}
      >
        {EXPORT_HISTORY_DOWNLOAD_LABEL}
      </a>
    );
  }
  if (facts.download.kind === "expired") {
    return <span>{EXPORT_HISTORY_EXPIRED_LABEL}</span>;
  }
  return <span>{EXPORT_HISTORY_DOWNLOAD_IDLE}</span>;
}

function HistoryRow({ record, now }: { record: WorkspaceExport; now: Date }): React.ReactElement {
  const facts = historyRowFacts(record, { now });
  return (
    <tr>
      <td>
        <Avatar
          name={facts.initiatorName}
          initial={facts.initiatorInitial}
          size="small"
          {...(facts.avatarUrl === null ? null : { src: facts.avatarUrl })}
        />
        <span>{facts.initiatorName}</span>
      </td>
      <td>
        <time dateTime={record.created_at}>{facts.startedLabel}</time>
      </td>
      <td>{facts.projectLabel}</td>
      <td>{facts.formatLabel}</td>
      <td>
        <Badge tone={facts.statusTone} dot>
          {facts.status}
        </Badge>
      </td>
      <td>
        <DownloadCell facts={facts} />
      </td>
    </tr>
  );
}

/**
 * Previous-exports history for one workspace. The refresh control
 * re-reads on demand, the pager follows the envelope's end flags and
 * cursors, and the table covers loading, empty, and failed reads.
 */
export function ExportsHistory({ workspaceSlug }: ExportsHistoryProps): React.ReactElement {
  const [cursor, setCursor] = React.useState(EXPORT_HISTORY_FIRST_CURSOR);
  const page = useExportsHistoryPage(workspaceSlug, cursor);
  const headingId = React.useId();
  const refreshing = page.isRefetching;
  const now = new Date();
  const data = page.data;
  const results = data?.results;

  let body: React.ReactNode;
  if (results === undefined) {
    body = page.isError ? (
      <p>{EXPORT_HISTORY_ERROR_MESSAGE}</p>
    ) : (
      <div role="status" aria-label={EXPORT_HISTORY_LOADING_LABEL}>
        <Skeleton height="40px" />
        <Skeleton height="40px" />
        <Skeleton height="40px" />
      </div>
    );
  } else if (results.length === 0) {
    body = (
      <EmptyState
        variant="illustrated"
        art={{ category: "general", type: "empty" }}
        title={EXPORT_HISTORY_EMPTY_TITLE}
        description={EXPORT_HISTORY_EMPTY_DESCRIPTION}
      />
    );
  } else {
    body = (
      <table aria-labelledby={headingId}>
        <thead>
          <tr>
            <th scope="col">Exported by</th>
            <th scope="col">Exported on</th>
            <th scope="col">Projects</th>
            <th scope="col">Format</th>
            <th scope="col">Status</th>
            <th scope="col">Download</th>
          </tr>
        </thead>
        <tbody>
          {results.map((record) => (
            <HistoryRow key={record.id} record={record} now={now} />
          ))}
        </tbody>
      </table>
    );
  }

  return (
    <section aria-labelledby={headingId}>
      <div>
        <div>
          <h3 id={headingId}>{EXPORT_HISTORY_TITLE}</h3>
          <Button
            type="button"
            size="small"
            loading={refreshing}
            onClick={() => {
              void page.refetch();
            }}
          >
            {refreshing ? EXPORT_HISTORY_REFRESH_BUSY : EXPORT_HISTORY_REFRESH_IDLE}
          </Button>
        </div>
        {data !== undefined && data.results.length > 0 ? (
          <div>
            <Button
              type="button"
              size="small"
              disabled={data.prev_page_results === false}
              onClick={() => {
                setCursor(data.prev_cursor);
              }}
            >
              Prev
            </Button>
            <Button
              type="button"
              size="small"
              disabled={data.next_page_results === false}
              onClick={() => {
                setCursor(data.next_cursor);
              }}
            >
              Next
            </Button>
          </div>
        ) : null}
      </div>
      {body}
    </section>
  );
}
