// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs screen container (NEWFRONT-545, RUN-016–022). One component serves
// the workspace and project scopes: it reads the page and selected run from
// the address, loads both through the area hooks, and composes the list and
// detail. Navigation writes the page with history-replace and keeps the
// page query when a row is selected.

import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import type { AgentRun } from "@pidash/api-client";
import { useRunnersArea } from "../area.js";
import { useCancelRun, useRun, useRuns } from "../api/runs.js";
import { RUN_CANCEL_FAILED, RUN_CANCEL_FAILED_TITLE, RunDetail } from "./RunDetail.js";
import { runsPageSearch } from "./runsModel.js";
import { RunsList } from "./RunsList.js";
import { notifyError } from "../../../shared/shell/index.js";

export interface RunsScreenProps {
  page: number;
  selectedRunId: string | null;
}

/** Scheduler detail path for a run's binding, or null when it has none. */
export function schedulerBindingHref(run: AgentRun): string | null {
  const binding = run.scheduler_binding_detail;
  if (binding === null || run.scheduler_binding === null) return null;
  return `/projects/${binding.project}/schedulers/${run.scheduler_binding}`;
}

export function RunsScreen({ page, selectedRunId }: RunsScreenProps): React.ReactElement {
  const area = useRunnersArea();
  const navigate = useNavigate();
  const list = useRuns(area.scope.workspaceSlug, area.workspaceId, area.scope.projectId ?? null, page);
  const detail = useRun(area.scope.workspaceSlug, selectedRunId);
  const cancel = useCancelRun(area.scope.workspaceSlug);

  function goToPage(next: number): void {
    void navigate({
      to: `${area.base}/runs`,
      search: runsPageSearch(next),
      replace: true,
    });
  }

  function selectRun(runId: string): void {
    void navigate({
      to: `${area.base}/runs/${runId}`,
      search: runsPageSearch(page),
      replace: true,
    });
  }

  function cancelRun(run: AgentRun): void {
    cancel.mutate(
      { runId: run.id, reason: "user" },
      {
        onError: () => notifyError(RUN_CANCEL_FAILED_TITLE, RUN_CANCEL_FAILED),
      }
    );
  }

  const state =
    selectedRunId === null ? "none" : detail.data !== undefined ? "loaded" : detail.isError ? "unavailable" : "loading";

  return (
    <div className="flex h-full min-h-0 flex-col">
      <RunsList data={list.data} loading={list.isPending} page={page} onPage={goToPage} onSelect={selectRun} />
      <div className="min-h-0 flex-1 overflow-auto border-t border-(--border) p-(--space-6)">
        <RunDetail
          state={state}
          run={detail.data ?? null}
          cancelling={cancel.isPending}
          onCancel={cancelRun}
          schedulerHref={(run) => {
            const href = schedulerBindingHref(run);
            return href === null ? null : `/${area.scope.workspaceSlug}${href}`;
          }}
        />
      </div>
    </div>
  );
}
