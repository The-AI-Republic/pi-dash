/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import type { IRunner, IRunnerLiveState } from "@pi-dash/types";
import { Badge, type TBadgeVariant } from "@pi-dash/ui";

/**
 * Per-active-run agent observability panel.
 *
 * Renders the descriptive scalars that ride on the runner's poll status
 * (``RunnerLiveState``) and derives an activity badge client-side. There
 * is intentionally no server-side ``agent_state`` enum — the badge is a
 * presentation concern, not authoritative lifecycle.
 *
 * See ``.ai_design/runner_agent_bridge/design.md`` §4.5.4.
 */

type ActivityBadgeKey = "active" | "thinking" | "stalled" | "dead" | "awaiting_approval" | "unknown";

interface RunnerAgentStatusPanelProps {
  runner: Pick<IRunner, "id" | "name" | "status">;
  liveState: IRunnerLiveState | null | undefined;
}

const BADGE_VARIANT: Record<ActivityBadgeKey, TBadgeVariant> = {
  active: "accent-success",
  thinking: "accent-warning",
  stalled: "accent-destructive",
  dead: "accent-destructive",
  awaiting_approval: "accent-primary",
  unknown: "accent-neutral",
};

const BADGE_LABEL: Record<ActivityBadgeKey, string> = {
  active: "Active",
  thinking: "Thinking",
  stalled: "Stalled",
  dead: "Subprocess dead",
  awaiting_approval: "Awaiting approval",
  unknown: "Unknown",
};

const ACTIVITY_THRESHOLD_SECS = 30;
const THINKING_THRESHOLD_SECS = 180;

/**
 * Pure derivation: compute the activity badge from the raw live-state
 * scalars + a "now" timestamp. Exported for testing.
 */
export function deriveActivityBadge(
  state: IRunnerLiveState | null | undefined,
  now: number = Date.now()
): ActivityBadgeKey {
  if (!state) return "unknown";

  if (state.agent_subprocess_alive === false) return "dead";

  if ((state.approvals_pending ?? 0) > 0) return "awaiting_approval";

  if (!state.last_event_at) return "unknown";

  const ageMs = now - new Date(state.last_event_at).getTime();
  if (Number.isNaN(ageMs) || ageMs < 0) return "unknown";

  const ageSecs = ageMs / 1000;
  if (ageSecs <= ACTIVITY_THRESHOLD_SECS) return "active";
  if (ageSecs <= THINKING_THRESHOLD_SECS) return "thinking";
  return "stalled";
}

function formatRelative(ts: string | null): string {
  if (!ts) return "—";
  const ms = Date.now() - new Date(ts).getTime();
  if (Number.isNaN(ms) || ms < 0) return "—";
  const secs = Math.round(ms / 1000);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.round(secs / 60)}m ago`;
  return `${Math.round(secs / 3600)}h ago`;
}

function formatNumber(n: number | null): string {
  if (n === null || n === undefined) return "—";
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

function formatBoolish(v: boolean | null): string {
  if (v === null || v === undefined) return "—";
  return v ? "yes" : "no";
}

export function RunnerAgentStatusPanel({ runner, liveState }: RunnerAgentStatusPanelProps) {
  // Re-render every 5s so the relative ages tick forward without needing
  // a fresh server fetch. Cheap; we only re-derive ageMs / labels.
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), 5_000);
    return () => clearInterval(id);
  }, []);

  const activityBadge = useMemo(
    () => deriveActivityBadge(liveState ?? null, Date.now()),
    // tick is a placeholder — value isn't read but keeps the badge fresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [liveState, tick]
  );

  const lastEventLabel = formatRelative(liveState?.last_event_at ?? null);
  const tokensLabel = liveState?.total_tokens != null ? formatNumber(liveState.total_tokens) : "—";

  return (
    <div className="border-custom-border-200 space-y-3 rounded border p-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-medium">Agent</h3>
          <Badge variant={BADGE_VARIANT[activityBadge]} size="sm">
            {BADGE_LABEL[activityBadge]}
          </Badge>
        </div>
        <span className="text-xs text-custom-text-300">{runner.name}</span>
      </div>

      <dl className="text-xs grid grid-cols-2 gap-x-4 gap-y-2">
        <dt className="text-custom-text-300">Last activity</dt>
        <dd className="font-mono" title={liveState?.last_event_summary ?? undefined}>
          {lastEventLabel}
        </dd>

        <dt className="text-custom-text-300">Last event</dt>
        <dd className="font-mono truncate">{liveState?.last_event_kind ?? "—"}</dd>

        <dt className="text-custom-text-300">Agent PID</dt>
        <dd className="font-mono">{liveState?.agent_pid ?? "—"}</dd>

        <dt className="text-custom-text-300">Subprocess alive</dt>
        <dd>{formatBoolish(liveState?.agent_subprocess_alive ?? null)}</dd>

        <dt className="text-custom-text-300">Approvals</dt>
        <dd>{liveState?.approvals_pending ?? 0}</dd>

        <dt className="text-custom-text-300">Tokens</dt>
        <dd className="font-mono">{tokensLabel}</dd>

        <dt className="text-custom-text-300">Turn</dt>
        <dd className="font-mono">{liveState?.turn_count != null ? liveState.turn_count : "—"}</dd>
      </dl>
    </div>
  );
}

export default RunnerAgentStatusPanel;
