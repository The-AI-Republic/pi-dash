/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import type { LucideIcon } from "lucide-react";
import { CircleAlert, CircleCheck, CirclePause, Clock3, LoaderCircle } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
// pi dash imports
import { Badge } from "@pi-dash/propel/badge";
import type { TBadgeVariant } from "@pi-dash/propel/badge";
import type { TAgentRunStatus, TIssue, TIssueAgentRunSummary, TIssueAgentTicker } from "@pi-dash/types";
import { cn } from "@pi-dash/utils";
// local imports
import type { TIssueOperations } from "./root";

type TranslationFn = ReturnType<typeof useTranslation>["t"];

type AgentStatusView = {
  title: string;
  detail: string | null;
  badge: string;
  badgeVariant: TBadgeVariant;
  icon: LucideIcon;
  iconClassName: string;
};

type Props = {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  issue: TIssue;
  issueOperations: TIssueOperations;
};

const ACTIVE_RUN_STATUSES = new Set<TAgentRunStatus>([
  "queued",
  "assigned",
  "running",
  "awaiting_approval",
  "awaiting_reauth",
  "paused_awaiting_input",
]);

function truncate(value: string, maxLength = 140): string {
  if (value.length <= maxLength) return value;
  return `${value.slice(0, maxLength - 3)}...`;
}

function formatRelativePast(timestamp: string | null | undefined, now: number, t: TranslationFn): string | null {
  if (!timestamp) return null;
  const diffMs = now - new Date(timestamp).getTime();
  if (Number.isNaN(diffMs) || diffMs < 0) return null;
  const minutes = Math.max(1, Math.round(diffMs / 60000));
  if (minutes < 60) return t("issue_agent_status.relative.minutes_ago", { count: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t("issue_agent_status.relative.hours_ago", { count: hours });
  return t("issue_agent_status.relative.days_ago", { count: Math.round(hours / 24) });
}

function formatUntil(timestamp: string | null | undefined, now: number, t: TranslationFn): string | null {
  if (!timestamp) return null;
  const diffMs = new Date(timestamp).getTime() - now;
  if (Number.isNaN(diffMs)) return null;
  if (diffMs <= 0) return t("issue_agent_status.relative.due_now");
  const minutes = Math.max(1, Math.ceil(diffMs / 60000));
  if (minutes < 60) return t("issue_agent_status.relative.minutes", { count: minutes });
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  if (hours < 24)
    return remainingMinutes > 0
      ? t("issue_agent_status.relative.hours_minutes", { hours, minutes: remainingMinutes })
      : t("issue_agent_status.relative.hours", { count: hours });
  const days = Math.floor(hours / 24);
  const remainingHours = hours % 24;
  return remainingHours > 0
    ? t("issue_agent_status.relative.days_hours", { days, hours: remainingHours })
    : t("issue_agent_status.relative.days", { count: days });
}

function formatRunDone(count: number, t: TranslationFn): string {
  const safeCount = Math.max(1, count);
  return t(safeCount === 1 ? "issue_agent_status.details.run_done_one" : "issue_agent_status.details.run_done_many", {
    count: safeCount,
  });
}

function formatTickBudget(ticker: TIssueAgentTicker | null | undefined, t: TranslationFn): string | null {
  if (!ticker) return null;
  if (ticker.max_ticks === -1) return t("issue_agent_status.details.tick_no_cap", { count: ticker.tick_count });
  return t("issue_agent_status.details.tick_budget", { count: ticker.tick_count, max: ticker.max_ticks });
}

function getPayloadString(payload: Record<string, unknown> | null | undefined, key: string): string | null {
  const value = payload?.[key];
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function getNestedPayloadString(
  payload: Record<string, unknown> | null | undefined,
  groupKey: string,
  key: string
): string | null {
  const group = payload?.[groupKey];
  if (!group || typeof group !== "object" || Array.isArray(group)) return null;
  return getPayloadString(group as Record<string, unknown>, key);
}

function getPayloadStringList(payload: Record<string, unknown> | null | undefined, key: string): string | null {
  const value = payload?.[key];
  if (!Array.isArray(value)) return null;
  const items = value.filter((item): item is string => typeof item === "string").map((item) => item.trim());
  const nonEmptyItems = items.filter(Boolean);
  return nonEmptyItems.length > 0 ? nonEmptyItems.join(", ") : null;
}

function getLiveDetail(run: TIssueAgentRunSummary, now: number, t: TranslationFn): string | null {
  const liveState = run.live_state;
  if (!liveState) return null;
  const lastActivity = formatRelativePast(liveState.last_event_at, now, t);
  if (liveState.last_event_summary && lastActivity) {
    return `${truncate(liveState.last_event_summary)} (${lastActivity})`;
  }
  if (liveState.last_event_summary) return truncate(liveState.last_event_summary);
  if (lastActivity) return t("issue_agent_status.details.last_activity", { time: lastActivity });
  return null;
}

function getRunView(
  run: TIssueAgentRunSummary,
  ticker: TIssueAgentTicker | null | undefined,
  runCount: number,
  now: number,
  t: TranslationFn
): AgentStatusView {
  const liveDetail = getLiveDetail(run, now, t);
  const runnerDetail = run.runner_name ? t("issue_agent_status.details.runner", { name: run.runner_name }) : null;
  const nextTick = formatUntil(ticker?.next_run_at, now, t);
  const doneDetail =
    ticker?.enabled && nextTick
      ? t("issue_agent_status.details.run_done_next_tick", {
          runs: formatRunDone(runCount, t),
          nextTick,
        })
      : formatRunDone(runCount, t);
  const pausedDetail =
    getNestedPayloadString(run.done_payload, "autonomy", "question_for_human") ??
    getPayloadString(run.done_payload, "summary");
  const blockedDetail =
    getPayloadStringList(run.done_payload, "blockers") ??
    getNestedPayloadString(run.done_payload, "autonomy", "reason") ??
    getPayloadString(run.done_payload, "summary");

  switch (run.status) {
    case "queued":
      return {
        title: t("issue_agent_status.titles.queued"),
        detail: runnerDetail ?? t("issue_agent_status.details.waiting_for_runner"),
        badge: t("issue_agent_status.badges.queued"),
        badgeVariant: "neutral",
        icon: Clock3,
        iconClassName: "text-tertiary",
      };
    case "assigned":
      return {
        title: t("issue_agent_status.titles.assigned"),
        detail: runnerDetail,
        badge: t("issue_agent_status.badges.assigned"),
        badgeVariant: "brand",
        icon: LoaderCircle,
        iconClassName: "animate-spin text-accent-primary",
      };
    case "running":
      return {
        title: t("issue_agent_status.titles.running"),
        detail: liveDetail ?? runnerDetail,
        badge: t("issue_agent_status.badges.running"),
        badgeVariant: "brand",
        icon: LoaderCircle,
        iconClassName: "animate-spin text-accent-primary",
      };
    case "awaiting_approval":
      return {
        title: t("issue_agent_status.titles.awaiting_approval"),
        detail: liveDetail ?? runnerDetail,
        badge: t("issue_agent_status.badges.awaiting_approval"),
        badgeVariant: "warning",
        icon: CirclePause,
        iconClassName: "text-warning-primary",
      };
    case "awaiting_reauth":
      return {
        title: t("issue_agent_status.titles.awaiting_reauth"),
        detail: runnerDetail,
        badge: t("issue_agent_status.badges.awaiting_reauth"),
        badgeVariant: "warning",
        icon: CircleAlert,
        iconClassName: "text-warning-primary",
      };
    case "paused_awaiting_input":
      return {
        title: t("issue_agent_status.titles.paused_awaiting_input"),
        detail: pausedDetail ? truncate(pausedDetail) : runnerDetail,
        badge: t("issue_agent_status.badges.paused_awaiting_input"),
        badgeVariant: "warning",
        icon: CirclePause,
        iconClassName: "text-warning-primary",
      };
    case "blocked":
      return {
        title: t("issue_agent_status.titles.blocked"),
        detail: blockedDetail ? truncate(blockedDetail) : runnerDetail,
        badge: t("issue_agent_status.badges.blocked"),
        badgeVariant: "warning",
        icon: CircleAlert,
        iconClassName: "text-warning-primary",
      };
    case "failed":
      return {
        title: t("issue_agent_status.titles.failed"),
        detail: run.error ? truncate(run.error) : t("issue_agent_status.details.latest_run_failed"),
        badge: t("issue_agent_status.badges.failed"),
        badgeVariant: "danger",
        icon: CircleAlert,
        iconClassName: "text-danger-primary",
      };
    case "cancelled":
      return {
        title: t("issue_agent_status.titles.cancelled"),
        detail: formatRelativePast(run.ended_at, now, t),
        badge: t("issue_agent_status.badges.cancelled"),
        badgeVariant: "neutral",
        icon: CirclePause,
        iconClassName: "text-tertiary",
      };
    case "completed":
    default:
      return {
        title: t("issue_agent_status.titles.completed"),
        detail: doneDetail,
        badge: t("issue_agent_status.badges.completed"),
        badgeVariant: "success",
        icon: CircleCheck,
        iconClassName: "text-success-primary",
      };
  }
}

function getTickerOnlyView(ticker: TIssueAgentTicker, now: number, t: TranslationFn): AgentStatusView {
  const nextTick = formatUntil(ticker.next_run_at, now, t);
  if (ticker.enabled) {
    return {
      title: t("issue_agent_status.titles.scheduled"),
      detail: nextTick
        ? t("issue_agent_status.details.next_tick_in", { nextTick })
        : t("issue_agent_status.details.waiting_for_next_tick"),
      badge: t("issue_agent_status.badges.scheduled"),
      badgeVariant: "brand",
      icon: Clock3,
      iconClassName: "text-accent-primary",
    };
  }

  if (ticker.user_disabled || ticker.disarm_reason === "user_disabled") {
    return {
      title: t("issue_agent_status.titles.disabled"),
      detail: t("issue_agent_status.details.disabled"),
      badge: t("issue_agent_status.badges.disabled"),
      badgeVariant: "neutral",
      icon: CirclePause,
      iconClassName: "text-tertiary",
    };
  }

  if (ticker.disarm_reason === "cap_hit") {
    return {
      title: t("issue_agent_status.titles.cap_hit"),
      detail: formatTickBudget(ticker, t),
      badge: t("issue_agent_status.badges.limit"),
      badgeVariant: "warning",
      icon: CircleAlert,
      iconClassName: "text-warning-primary",
    };
  }

  return {
    title: t("issue_agent_status.titles.off"),
    detail: formatTickBudget(ticker, t),
    badge: t("issue_agent_status.badges.off"),
    badgeVariant: "neutral",
    icon: CirclePause,
    iconClassName: "text-tertiary",
  };
}

function getAgentStatusView(issue: TIssue, now: number, t: TranslationFn): AgentStatusView | null {
  const status = issue.agent_status;
  const ticker = status?.ticker ?? issue.agent_ticker;
  const run = status?.active_run ?? status?.latest_run;

  if (run) return getRunView(run, ticker, status?.run_count ?? 1, now, t);
  if (ticker) return getTickerOnlyView(ticker, now, t);
  return null;
}

export function IssueAgentStatusPanel({ workspaceSlug, projectId, issueId, issue, issueOperations }: Props) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  const status = issue.agent_status;
  const ticker = status?.ticker ?? issue.agent_ticker;
  const activeRun = status?.active_run;
  const activeRunId = activeRun?.id;
  const activeRunStatus = activeRun?.status;
  const shouldPollAgentStatus = Boolean(
    ticker?.enabled || (activeRunStatus && ACTIVE_RUN_STATUSES.has(activeRunStatus))
  );
  const view = useMemo(() => getAgentStatusView(issue, now, t), [issue, now, t]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!shouldPollAgentStatus) return;
    void issueOperations.fetch(workspaceSlug, projectId, issueId);
    const timer = window.setInterval(() => {
      void issueOperations.fetch(workspaceSlug, projectId, issueId);
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [activeRunId, issueId, issueOperations, projectId, shouldPollAgentStatus, workspaceSlug]);

  if (!view) return null;

  const Icon = view.icon;
  const nextTick = formatUntil(ticker?.next_run_at, now, t);
  const tickBudget = formatTickBudget(ticker, t);

  return (
    <section className="mt-5 rounded border border-subtle bg-surface-2 px-3 py-3">
      <div className="flex items-start gap-2.5">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-sm bg-layer-2">
          <Icon className={cn("size-4", view.iconClassName)} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <h5 className="truncate text-body-xs-medium text-primary">{view.title}</h5>
            <Badge variant={view.badgeVariant} size="sm">
              {view.badge}
            </Badge>
          </div>
          {view.detail && <p className="mt-1 line-clamp-2 text-body-xs-regular text-tertiary">{view.detail}</p>}
        </div>
      </div>

      {(status?.run_count || nextTick || tickBudget) && (
        <div className="mt-3 grid grid-cols-2 gap-2 text-caption-sm-medium text-tertiary">
          {status?.run_count ? (
            <div className="rounded-sm bg-layer-2 px-2 py-1">
              <span className="block text-placeholder">{t("issue_agent_status.stats.runs")}</span>
              <span className="text-primary">{status.run_count}</span>
            </div>
          ) : null}
          {nextTick ? (
            <div className="rounded-sm bg-layer-2 px-2 py-1">
              <span className="block text-placeholder">{t("issue_agent_status.stats.next_tick")}</span>
              <span className="text-primary">{nextTick}</span>
            </div>
          ) : null}
          {tickBudget ? (
            <div className="rounded-sm bg-layer-2 px-2 py-1">
              <span className="block text-placeholder">{t("issue_agent_status.stats.budget")}</span>
              <span className="text-primary">{tickBudget}</span>
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}
