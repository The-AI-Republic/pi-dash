// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent status reads for the issue detail (parity/specs/issues.md §22,
// ISS-164). Pure: which run the card speaks for, the headline triple
// (title, detail, badge) for every run and ticker state, the budget and
// next-tick tiles, and the cumulative token total that folds live
// in-flight usage into the stored server sum without double-counting
// the active row's own figure. The panel component owns polling and
// the re-tick/abort writes; everything decidable from one issue read
// lives here so tests pin it without mounting anything.
import type { BadgeTone } from "@pidash/kit";
import type { AgentRunSummary, AgentStatus, AgentTicker, Issue } from "@pidash/api-client";

/** Live poll while a run is in flight. */
export const AGENT_STATUS_POLL_MS = 15_000;

/** Largest delay a scheduled refresh may ask the timer for. */
export const MAX_SCHEDULED_REFRESH_MS = 2_147_483_647;

/** Run states that keep the card polling. */
const ACTIVE_RUN_STATUSES: ReadonlySet<string> = new Set([
  "queued",
  "assigned",
  "running",
  "cancel_requested",
  "awaiting_approval",
  "awaiting_reauth",
  "paused_awaiting_input",
]);

/** Longest detail line the card shows before cutting it short. */
const DETAIL_LIMIT = 140;

/** The card's headline: title, second line, and badge. */
export interface AgentStatusView {
  title: string;
  detail: string | null;
  badge: string;
  tone: BadgeTone;
  /** Which glyph the panel renders beside the title. */
  icon: "clock" | "spinner" | "pause" | "alert" | "check";
}

/** True while the given run state is still in flight. */
export function isActiveRunStatus(status: string): boolean {
  return ACTIVE_RUN_STATUSES.has(status);
}

/** The run the card speaks for: the live one, else the most recent. */
export function selectDisplayRun(status: AgentStatus | null | undefined): AgentRunSummary | null {
  return status?.active_run ?? status?.latest_run ?? null;
}

/** The issue's continuation clock, wherever the payload carries it. */
export function selectAgentTicker(issue: Pick<Issue, "agent_status" | "agent_ticker">): AgentTicker | null {
  return issue.agent_status?.ticker ?? issue.agent_ticker ?? null;
}

/** The card renders only with a run or a ticker behind it. */
export function agentPanelVisible(issue: Pick<Issue, "agent_status" | "agent_ticker">): boolean {
  return selectDisplayRun(issue.agent_status) !== null || selectAgentTicker(issue) !== null;
}

/** The run an abort would stop: live, and not already stopping. */
export function selectAbortableRun(status: AgentStatus | null | undefined): AgentRunSummary | null {
  const run = status?.active_run ?? null;
  if (run === null || !isActiveRunStatus(run.status) || run.status === "cancel_requested") return null;
  return run;
}

function truncate(value: string): string {
  if (value.length <= DETAIL_LIMIT) return value;
  return `${value.slice(0, DETAIL_LIMIT - 3)}...`;
}

function payloadText(payload: unknown, key: string): string | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return null;
  const value = (payload as Record<string, unknown>)[key];
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function nestedPayloadText(payload: unknown, group: string, key: string): string | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return null;
  return payloadText((payload as Record<string, unknown>)[group], key);
}

function payloadTextList(payload: unknown, key: string): string | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return null;
  const value = (payload as Record<string, unknown>)[key];
  if (!Array.isArray(value)) return null;
  const items = value.filter((entry): entry is string => typeof entry === "string").map((entry) => entry.trim());
  const kept = items.filter((entry) => entry !== "");
  return kept.length === 0 ? null : kept.join(", ");
}

/** "5m ago" style age, or null when the stamp is missing or ahead. */
export function formatAge(timestamp: string | null | undefined, nowMs: number): string | null {
  if (timestamp === null || timestamp === undefined) return null;
  const ageMs = nowMs - new Date(timestamp).getTime();
  if (Number.isNaN(ageMs) || ageMs < 0) return null;
  const minutes = Math.max(1, Math.round(ageMs / 60_000));
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

/** "12m" style countdown, "due now" once reached, null without a stamp. */
export function formatCountdown(timestamp: string | null | undefined, nowMs: number): string | null {
  if (timestamp === null || timestamp === undefined) return null;
  const restMs = new Date(timestamp).getTime() - nowMs;
  if (Number.isNaN(restMs)) return null;
  if (restMs <= 0) return "due now";
  const minutes = Math.max(1, Math.ceil(restMs / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const spareMinutes = minutes % 60;
  if (hours < 24) return spareMinutes === 0 ? `${hours}h` : `${hours}h ${spareMinutes}m`;
  const days = Math.floor(hours / 24);
  const spareHours = hours % 24;
  return spareHours === 0 ? `${days}d` : `${days}d ${spareHours}h`;
}

/**
 * The budget tile: used-over-max runs, with the waits that bought ticks
 * back counted beside it. A -1 cap reads as uncapped; null without a
 * ticker at all.
 */
export function formatTickBudget(ticker: AgentTicker | null | undefined): string | null {
  if (ticker === null || ticker === undefined) return null;
  const used = ticker.used;
  const waited = ticker.waited;
  if (ticker.max_ticks === -1) return `${used} runs used, uncapped`;
  const budget = `${used}/${ticker.max_ticks} runs used`;
  if (waited === 0) return budget;
  return `${budget} (+${waited} ${waited === 1 ? "wait" : "waits"})`;
}

/**
 * The next-tick tile. A queued entry reads as queued rather than a time;
 * a disarmed clock reads as nothing even though it keeps its stamp, so
 * the gate is the armed flag, not the timestamp existing.
 */
export function formatNextTick(ticker: AgentTicker | null | undefined, nowMs: number): string | null {
  if (ticker === null || ticker === undefined) return null;
  if (ticker.pending_entry) return "queued";
  if (!ticker.enabled) return null;
  return formatCountdown(ticker.next_run_at, nowMs);
}

function runsDoneText(runCount: number): string {
  const count = Math.max(1, runCount);
  return count === 1 ? "1 run finished" : `${count} runs finished`;
}

function liveDetail(run: AgentRunSummary, nowMs: number): string | null {
  const live = run.live_state;
  if (live === null) return null;
  const age = formatAge(live.last_event_at, nowMs);
  const raw = live.last_event_summary;
  const summary = raw === null || raw.trim() === "" ? null : truncate(raw);
  if (summary !== null && age !== null) return `${summary} (${age})`;
  if (summary !== null) return summary;
  if (age !== null) return `Latest activity ${age}`;
  return null;
}

/** Headline for one run plus the ticker behind it. */
export function runStatusView(
  run: AgentRunSummary,
  ticker: AgentTicker | null,
  runCount: number,
  nowMs: number
): AgentStatusView {
  const live = liveDetail(run, nowMs);
  const runner = run.runner_name === null ? null : `Runner ${run.runner_name}`;
  const armed = ticker !== null && ticker.enabled;
  const countdown = ticker === null ? null : formatCountdown(ticker.next_run_at, nowMs);
  const finished =
    armed && countdown !== null ? `${runsDoneText(runCount)}, next run ${countdown}` : runsDoneText(runCount);
  const paused =
    nestedPayloadText(run.done_payload, "autonomy", "question_for_human") ?? payloadText(run.done_payload, "summary");
  const blocked =
    payloadTextList(run.done_payload, "blockers") ??
    nestedPayloadText(run.done_payload, "autonomy", "reason") ??
    payloadText(run.done_payload, "summary");

  switch (run.status) {
    case "queued":
      return {
        title: "Agent run queued",
        detail:
          run.executor_kind === "cloud_agent"
            ? "Waiting for cloud agent capacity."
            : (runner ?? "Waiting for a free runner."),
        badge: "Queued",
        tone: "neutral",
        icon: "clock",
      };
    case "assigned":
      return {
        title: "Agent run starting",
        detail: runner,
        badge: "Starting",
        tone: "accent",
        icon: "spinner",
      };
    case "running":
      return {
        title: "Agent at work",
        detail:
          run.executor_kind === "cloud_agent"
            ? "The cloud agent is working with the project's tools."
            : (live ?? runner),
        badge: "Working",
        tone: "accent",
        icon: "spinner",
      };
    case "cancel_requested":
      return {
        title: "Stopping the agent run",
        detail: runner ?? "The runner is confirming the stop.",
        badge: "Stopping",
        tone: "warning",
        icon: "spinner",
      };
    case "awaiting_approval":
      return {
        title: "Agent run needs approval",
        detail: live ?? runner,
        badge: "Approval",
        tone: "warning",
        icon: "pause",
      };
    case "awaiting_reauth":
      return {
        title: "Agent run needs fresh credentials",
        detail: runner,
        badge: "Re-auth",
        tone: "warning",
        icon: "alert",
      };
    case "paused_awaiting_input":
      return {
        title: "Agent run paused for input",
        detail: paused === null ? runner : truncate(paused),
        badge: "Paused",
        tone: "warning",
        icon: "pause",
      };
    case "blocked":
      return {
        title: "Agent run blocked",
        detail: blocked === null ? runner : truncate(blocked),
        badge: "Blocked",
        tone: "warning",
        icon: "alert",
      };
    case "failed": {
      const diagnostic = run.error_diagnostic;
      const failure =
        diagnostic !== null
          ? `${diagnostic.source_label}: ${truncate(diagnostic.summary)}`
          : run.error !== ""
            ? truncate(run.error)
            : "The latest run did not finish.";
      return { title: "Agent run failed", detail: failure, badge: "Failed", tone: "danger", icon: "alert" };
    }
    case "cancelled":
      return {
        title: "Agent run cancelled",
        detail: formatAge(run.ended_at, nowMs),
        badge: "Cancelled",
        tone: "neutral",
        icon: "pause",
      };
    case "refused":
      return {
        title: "Agent run refused",
        detail: run.error !== "" ? truncate(run.error) : "The provider declined this run.",
        badge: "Refused",
        tone: "danger",
        icon: "alert",
      };
    case "completed":
      // A self-stopped clock always has the run that stopped it, so the
      // run view — not the ticker view — carries the stop below.
      if (ticker !== null && !ticker.enabled && ticker.disarm_reason === "stop_signal") {
        return {
          title: "Agent ticking stopped",
          detail: `${runsDoneText(runCount)} — the agent stopped its own schedule. A comment, a state change, or Run AI restarts it.`,
          badge: "Stopped",
          tone: "neutral",
          icon: "pause",
        };
      }
      return {
        title: "Agent run finished",
        detail: finished,
        badge: "Done",
        tone: "success",
        icon: "check",
      };
    default: {
      // A retired status the union no longer names still renders: the
      // run never started, so it must not read as a success. The raw
      // status carries the badge instead.
      const retired = String(run.status);
      return {
        title: "Agent run in a retired state",
        detail: runner ?? `Reported status: ${retired}`,
        badge: retired,
        tone: "neutral",
        icon: "clock",
      };
    }
  }
}

/** Headline when only the clock exists, with no run behind it. */
export function tickerStatusView(ticker: AgentTicker, nowMs: number): AgentStatusView {
  const countdown = formatCountdown(ticker.next_run_at, nowMs);
  if (ticker.enabled) {
    return {
      title: "Agent ticking scheduled",
      detail: countdown === null ? "Waiting for the next scheduled run." : `Next run ${countdown}`,
      badge: "Scheduled",
      tone: "accent",
      icon: "clock",
    };
  }
  if (ticker.user_disabled || ticker.disarm_reason === "user_disabled") {
    return {
      title: "Agent ticking switched off",
      detail: "Automatic runs are off for this issue.",
      badge: "Disabled",
      tone: "neutral",
      icon: "pause",
    };
  }
  if (ticker.disarm_reason === "stop_signal") {
    return {
      title: "Agent ticking stopped",
      detail: "The agent stopped its own schedule. A comment, a state change, or Run AI restarts it.",
      badge: "Stopped",
      tone: "neutral",
      icon: "pause",
    };
  }
  if (ticker.disarm_reason === "cap_hit" || ticker.disarm_reason === "pool_spent") {
    return {
      title: "Agent run budget spent",
      detail: formatTickBudget(ticker),
      badge: "Limit",
      tone: "warning",
      icon: "alert",
    };
  }
  return {
    title: "Agent ticking off",
    detail: formatTickBudget(ticker),
    badge: "Off",
    tone: "neutral",
    icon: "pause",
  };
}

/** The card's headline, or null when nothing backs the card. */
export function agentStatusView(
  issue: Pick<Issue, "agent_status" | "agent_ticker">,
  nowMs: number
): AgentStatusView | null {
  const status = issue.agent_status;
  const ticker = selectAgentTicker(issue);
  const run = selectDisplayRun(status);
  const runCount = status?.run_count ?? 1;
  if (run !== null) return runStatusView(run, ticker, runCount, nowMs);
  if (ticker !== null) return tickerStatusView(ticker, nowMs);
  return null;
}

/** Executor tile: the cloud agent, or the project's own runner. */
export function executorLabel(executorKind: string): string {
  return executorKind === "cloud_agent" ? "Cloud agent" : "Local runner";
}

/**
 * Cumulative token total for the tile: the stored server sum plus only
 * what the live heartbeat reports beyond the active row's own figure —
 * the whole live figure while the row is unwritten, just the delta
 * while it lags, nothing once it caught up. Null hides the tile, and
 * that happens only when the payload carries no token figure at all
 * (neither a stored sum nor live usage); a reported zero still shows.
 */
export function selectTokenTotal(status: AgentStatus | null | undefined): number | null {
  const stored = status?.total_tokens ?? null;
  const active = status?.active_run ?? null;
  const live = active?.live_state?.total_tokens ?? null;
  if (stored === null && live === null) return null;
  const row = active?.total_tokens ?? null;
  const extra = live === null ? 0 : row === null ? live : Math.max(0, live - row);
  return (stored ?? 0) + extra;
}

/** Compact token figure: verbatim below a thousand, then 12.8k / 1.2M. */
export function formatTokenCount(value: number): string {
  if (value < 1_000) return String(value);
  if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}k`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}
