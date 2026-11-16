// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent status panel for the issue detail (parity/specs/issues.md §22,
// ISS-164). Reads the issue's agent runs and continuation clock through
// the pure view model, polls the issue read every fifteen seconds while
// a run is in flight, and refreshes once at the next tick while idle.
// Re-tick and abort ride the runners dispatch hooks; the panel only
// decides when they show. Renders nothing without a run or a ticker.
import { Badge, Button, Dialog, Tooltip } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { CircleCheck, Clock, LoaderCircle, OctagonX, Pause, RotateCw, TriangleAlert } from "lucide-react";
import * as React from "react";

import { useAbortAgentRun, useReTick } from "../../../runners/index.js";
import {
  AGENT_STATUS_POLL_MS,
  agentStatusView,
  executorLabel,
  formatNextTick,
  formatTickBudget,
  formatTokenCount,
  isActiveRunStatus,
  MAX_SCHEDULED_REFRESH_MS,
  selectAbortableRun,
  selectAgentTicker,
  selectTokenTotal,
  type AgentStatusView,
} from "./agentStatus.js";

export interface AgentStatusPanelProps {
  workspaceSlug: string;
  issue: Issue;
  /** Re-read the issue (the retrieve carries the agent-status token total). */
  refresh: () => Promise<unknown>;
}

/** How often the relative labels re-render without a server round-trip. */
const CLOCK_TICK_MS = 60_000;

const VIEW_ICONS = {
  clock: Clock,
  spinner: LoaderCircle,
  pause: Pause,
  alert: TriangleAlert,
  check: CircleCheck,
} as const;

/** Why the token figure understates reality, in the tile's tooltip. */
export const TOKENS_FLOOR_NOTE =
  "Runs that ended before reporting count as nothing, so this total is a floor rather than a bill.";

function viewIcon(view: AgentStatusView): React.ReactElement {
  const Icon = VIEW_ICONS[view.icon];
  const spinning = view.icon === "spinner";
  return (
    <span className="flex size-7 shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle)">
      <Icon
        size={16}
        strokeWidth={1.5}
        aria-hidden="true"
        className={spinning ? "animate-spin text-(--accent)" : "text-(--text-muted)"}
      />
    </span>
  );
}

export function AgentStatusPanel({ workspaceSlug, issue, refresh }: AgentStatusPanelProps): React.ReactElement | null {
  const [now, setNow] = React.useState(() => Date.now());
  const [confirmAbort, setConfirmAbort] = React.useState(false);
  const status = issue.agent_status;
  const ticker = selectAgentTicker(issue);
  const activeRun = status?.active_run ?? null;
  const hasActiveRun = activeRun !== null && isActiveRunStatus(activeRun.status);
  const abortable = selectAbortableRun(status);
  const view = React.useMemo(() => agentStatusView(issue, now), [issue, now]);
  const canReTick = ticker?.can_re_tick === true;
  const { reTick, isSubmitting: reTicking } = useReTick({ onSettledRefresh: refresh });
  const abort = useAbortAgentRun(workspaceSlug, { onSettledRefresh: refresh });

  React.useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, []);

  React.useEffect(() => {
    if (!hasActiveRun) return;
    const timer = window.setInterval(() => {
      void refresh();
    }, AGENT_STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, [hasActiveRun, refresh]);

  React.useEffect(() => {
    if (hasActiveRun || ticker?.enabled !== true) return;
    const next = ticker.next_run_at;
    if (next === null) return;
    const at = new Date(next).getTime();
    if (Number.isNaN(at)) return;
    const delay = Math.min(Math.max(at - Date.now(), 0), MAX_SCHEDULED_REFRESH_MS);
    const timer = window.setTimeout(() => {
      void refresh();
    }, delay);
    return () => window.clearTimeout(timer);
  }, [hasActiveRun, refresh, ticker]);

  if (view === null) return null;

  const runCount = status?.run_count ?? 0;
  const latestRun = status?.latest_run ?? null;
  const executor = latestRun === null ? null : executorLabel(latestRun.executor_kind);
  const nextTick = formatNextTick(ticker, now);
  const budget = formatTickBudget(ticker);
  const tokens = selectTokenTotal(status);
  const hasTiles = runCount > 0 || executor !== null || nextTick !== null || budget !== null || tokens !== null;

  const confirmAbortRun = (): void => {
    // The run may have landed while the dialog stood open; then there
    // is nothing left to stop and the dialog just closes.
    const target = selectAbortableRun(status);
    if (target === null) {
      setConfirmAbort(false);
      return;
    }
    void abort.mutateAsync({ runId: target.id }).then(
      () => setConfirmAbort(false),
      () => undefined
    );
  };

  return (
    <section aria-label="Agent status" data-testid="agent-status-panel" className="flex flex-col gap-(--space-3)">
      <div className="flex items-start gap-(--space-2)">
        {viewIcon(view)}
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-(--space-2)">
            <p className="text-body truncate font-medium text-(--text)">{view.title}</p>
            <Badge tone={view.tone}>{view.badge}</Badge>
          </div>
          {view.detail === null ? null : (
            <p className="text-body mt-(--space-1) line-clamp-2 text-(--text-muted)">{view.detail}</p>
          )}
        </div>
      </div>

      {hasTiles ? (
        <dl className="grid grid-cols-2 gap-(--space-2)">
          {runCount > 0 ? (
            <div className="rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)">
              <dt className="text-meta text-(--text-muted)">Runs</dt>
              <dd className="text-body text-(--text)">{runCount}</dd>
            </div>
          ) : null}
          {executor === null ? null : (
            <div className="rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)">
              <dt className="text-meta text-(--text-muted)">Executor</dt>
              <dd className="text-body text-(--text)">{executor}</dd>
            </div>
          )}
          {nextTick === null ? null : (
            <div className="rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)">
              <dt className="text-meta text-(--text-muted)">Next tick</dt>
              <dd className="text-body text-(--text)">{nextTick}</dd>
            </div>
          )}
          {budget === null ? null : (
            <div className="rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)">
              <dt className="text-meta text-(--text-muted)">Budget</dt>
              <dd className="text-body text-(--text)">{budget}</dd>
            </div>
          )}
          {tokens === null ? null : (
            <div className="rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1)">
              <dt className="text-meta text-(--text-muted)">Tokens</dt>
              <dd className="text-body text-(--text)">
                <Tooltip content={TOKENS_FLOOR_NOTE}>
                  <span>{formatTokenCount(tokens)}</span>
                </Tooltip>
              </dd>
            </div>
          )}
        </dl>
      ) : null}

      {canReTick ? (
        <div className="flex flex-col gap-(--space-1)">
          <Button
            variant="secondary"
            size="small"
            disabled={reTicking}
            loading={reTicking}
            data-testid="issue-re-tick"
            onClick={() => void reTick(issue.id)}
          >
            <span className="flex items-center gap-(--space-1)">
              <RotateCw size={14} strokeWidth={1.5} aria-hidden="true" />
              <span>Re-tick</span>
            </span>
          </Button>
          <p className="text-meta text-(--text-muted)">Adds runs to this issue's budget and starts the agent.</p>
        </div>
      ) : null}

      {abortable === null ? null : (
        <div className="flex flex-col gap-(--space-1)">
          <Button
            variant="danger"
            size="small"
            disabled={abort.isPending}
            loading={abort.isPending}
            onClick={() => setConfirmAbort(true)}
          >
            <span className="flex items-center gap-(--space-1)">
              <OctagonX size={14} strokeWidth={1.5} aria-hidden="true" />
              <span>Abort run</span>
            </span>
          </Button>
          <p className="text-meta text-(--text-muted)">Asks the runner to stop the current run.</p>
        </div>
      )}

      <Dialog
        open={confirmAbort}
        // Dismissal needs no pending guard: nonDismissable already holds
        // Escape, the backdrop, and the close control while aborting.
        onOpenChange={() => setConfirmAbort(false)}
        title="Abort run?"
        description="The runner stops this run as soon as it receives the signal."
        nonDismissable={abort.isPending}
        actions={
          <>
            <Button disabled={abort.isPending} onClick={() => setConfirmAbort(false)}>
              Cancel
            </Button>
            <Button variant="danger" loading={abort.isPending} onClick={confirmAbortRun}>
              {abort.isPending ? "Aborting…" : "Abort run"}
            </Button>
          </>
        }
      >
        <p className="text-body text-(--text-muted)">The run moves to cancelled once the runner confirms the stop.</p>
      </Dialog>
    </section>
  );
}
