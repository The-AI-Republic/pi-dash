// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paginated runs list (NEWFRONT-545, RUN-016). The page lives in the
// address, every row preserves the page query when selected, and the
// footer turns pages with history-replace so Back leaves the tab.

import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import type { AgentRunPage } from "@pidash/api-client";
import { pageLabel, startedLabel } from "./runsModel.js";
import { RunStatusBadge } from "./RunStatusBadge.js";

export interface RunsListProps {
  data: AgentRunPage | undefined;
  loading: boolean;
  page: number;
  onPage: (page: number) => void;
  onSelect: (runId: string) => void;
}

export const RUNS_EMPTY_TEXT = "No runs yet.";

export function RunsList({ data, loading, page, onPage, onSelect }: RunsListProps): React.ReactElement {
  const runs = data?.results ?? [];
  const totalCount = data?.total_count ?? 0;
  const totalPages = Math.max(1, data?.total_pages ?? 1);
  return (
    <section aria-label="Runs" data-testid="runs-list" className="flex min-h-0 flex-1 flex-col">
      <h1 className="text-h2 px-(--space-6) pt-(--space-6) text-(--text)">Runs</h1>
      <div className="mt-(--space-4) min-h-0 flex-1 overflow-auto px-(--space-6)">
        <div className="overflow-hidden rounded-(--radius-control) border border-(--border)">
          <table data-testid="runs-table" className="text-body w-full text-left">
            <thead className="bg-(--subtle) text-(--text-muted)">
              <tr>
                <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                  Started
                </th>
                <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                  Status
                </th>
                <th scope="col" className="px-(--space-3) py-(--space-2) font-medium">
                  Prompt
                </th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <tr
                  key={run.id}
                  data-testid="runs-row"
                  data-run-id={run.id}
                  tabIndex={0}
                  onClick={() => onSelect(run.id)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onSelect(run.id);
                    }
                  }}
                  className="cursor-pointer border-t border-(--border) hover:bg-(--subtle)"
                >
                  <td className="px-(--space-3) py-(--space-2) whitespace-nowrap">{startedLabel(run)}</td>
                  <td className="px-(--space-3) py-(--space-2)" data-testid="runs-row-status">
                    <RunStatusBadge status={run.status} />
                  </td>
                  <td
                    className="font-mono max-w-56 truncate px-(--space-3) py-(--space-2) text-(--text-muted)"
                    data-testid="runs-row-prompt"
                  >
                    {run.prompt}
                  </td>
                </tr>
              ))}
              {runs.length === 0 ? (
                <tr>
                  <td colSpan={3} className="px-(--space-3) py-(--space-8) text-center text-(--text-muted)">
                    {loading ? <Spinner size="small" /> : RUNS_EMPTY_TEXT}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
      </div>
      {totalCount > 0 ? (
        <footer
          data-testid="runs-pager"
          className="mt-(--space-4) flex items-center justify-between gap-(--space-2) border-t border-(--border) px-(--space-6) py-(--space-2)"
        >
          <span className="text-meta text-(--text-muted)" data-testid="runs-page-label">
            {pageLabel(page, totalPages)}
          </span>
          <div className="flex items-center gap-(--space-2)">
            <Button size="small" disabled={page <= 1} onClick={() => onPage(page - 1)}>
              Previous
            </Button>
            <Button size="small" disabled={page >= totalPages} onClick={() => onPage(page + 1)}>
              Next
            </Button>
          </div>
        </footer>
      ) : null}
    </section>
  );
}
