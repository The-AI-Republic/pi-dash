// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent activity panel (NEWFRONT-555, RUN-043). Renders the runner's live
// snapshot as a telemetry grid and derives its own badge. The panel ticks on
// its own interval, so the relative age and the badge age visibly between
// server polls.

import { Badge } from "@pidash/kit";
import type { RunnerLiveState } from "@pidash/api-client";
import * as React from "react";

import {
  ACTIVITY_LABELS,
  ACTIVITY_TICK_MS,
  ACTIVITY_TONES,
  activityBadge,
  compactCount,
  livenessLabel,
  relativeAge,
} from "./activity.js";

export const ACTIVITY_HEADING = "Agent";
export const ACTIVITY_EMPTY_LABEL = "—";

export interface ActivityPanelProps {
  runnerName: string;
  liveState: RunnerLiveState | null | undefined;
  /** Tick interval; tests shorten it to observe aging. */
  tickMs?: number | undefined;
  /** Clock injection for deterministic tests. */
  now?: (() => number) | undefined;
}

export function ActivityPanel({ runnerName, liveState, tickMs, now }: ActivityPanelProps): React.ReactElement {
  const clock = now ?? Date.now;
  const interval = tickMs ?? ACTIVITY_TICK_MS;
  // The tick writes the wall clock into state: a bare counter leaves the
  // age expression memoizable on props alone, so the panel would never
  // re-render its relative age (NEWFRONT-557).
  const [moment, setMoment] = React.useState(() => clock());
  const clockRef = React.useRef(clock);
  clockRef.current = clock;

  React.useEffect(() => {
    const id = setInterval(() => setMoment(clockRef.current()), interval);
    return () => clearInterval(id);
  }, [interval]);
  const badge = activityBadge(liveState ?? null, moment);
  const state = liveState ?? null;

  return (
    <section
      aria-label="Agent activity"
      className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)"
    >
      <div className="flex items-center justify-between gap-(--space-3)">
        <div className="flex items-center gap-(--space-2)">
          <h2 className="text-emphasis font-medium text-(--text)">{ACTIVITY_HEADING}</h2>
          <span data-testid="activity-status-badge" className="inline-flex">
            <Badge tone={ACTIVITY_TONES[badge]} dot>
              {ACTIVITY_LABELS[badge]}
            </Badge>
          </span>
        </div>
        <span className="text-meta text-(--text-muted)">{runnerName}</span>
      </div>
      <dl className="text-meta grid grid-cols-[max-content_1fr] gap-x-(--space-4) gap-y-(--space-2)">
        <dt className="text-(--text-muted)">Last activity</dt>
        <dd className="font-mono text-(--text)" title={state?.last_event_summary ?? undefined}>
          {relativeAge(state?.last_event_at ?? null, moment)}
        </dd>
        <dt className="text-(--text-muted)">Last event</dt>
        <dd className="font-mono truncate text-(--text)">{state?.last_event_kind ?? ACTIVITY_EMPTY_LABEL}</dd>
        <dt className="text-(--text-muted)">Agent PID</dt>
        <dd className="font-mono text-(--text)">{state?.agent_pid ?? ACTIVITY_EMPTY_LABEL}</dd>
        <dt className="text-(--text-muted)">Subprocess alive</dt>
        <dd className="text-(--text)">{livenessLabel(state?.agent_subprocess_alive)}</dd>
        <dt className="text-(--text-muted)">Approvals</dt>
        <dd className="text-(--text)">{state?.approvals_pending ?? 0}</dd>
        <dt className="text-(--text-muted)">Tokens</dt>
        <dd className="font-mono text-(--text)">
          {state?.total_tokens == null ? ACTIVITY_EMPTY_LABEL : compactCount(state.total_tokens)}
        </dd>
        <dt className="text-(--text-muted)">Model</dt>
        <dd className="font-mono truncate text-(--text)" title={modelTitle(state)}>
          {modelLabel(state)}
        </dd>
        <dt className="text-(--text-muted)">Turn</dt>
        <dd className="font-mono text-(--text)">{state?.turn_count ?? ACTIVITY_EMPTY_LABEL}</dd>
      </dl>
    </section>
  );
}

function modelLabel(state: RunnerLiveState | null): string {
  const model = state?.llm_model?.trim() ?? "";
  return model === "" ? ACTIVITY_EMPTY_LABEL : model;
}

function modelTitle(state: RunnerLiveState | null): string | undefined {
  const model = state?.llm_model?.trim() ?? "";
  return model === "" ? undefined : model;
}
