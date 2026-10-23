// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Presentation rules for one install's detail page
// (parity/specs/project-settings-part-f.md §PSET-059; parity/specs/agents.md
// §3 AGT-013). Pure: the config rows, the run-history row text, the empty
// history sentence and the completion badge all read from the API row, so
// the page component stays layout only. Two labels are read back by the
// parity driver; they are constants so both stay one string.

import type { AgentRun, SchedulerBindingDetail, WorkspaceMember } from "@pidash/api-client";

import { describeRrule } from "./schedule.js";

/**
 * The scheduler a definition came from. Renders the raw API value the
 * catalog and install-detail surfaces both show, so origin marks read the
 * same wherever an install appears.
 */
export function sourceLabel(source: SchedulerBindingDetail["scheduler_source"]): string {
  return source;
}

/** Displayed time zone; an install with no zone fires on the server default. */
export function displayTimeZone(tzid: string): string {
  return tzid.trim() === "" ? "UTC" : tzid;
}

/** Locale timestamp, or the never mark the table shares. */
export function formatMoment(value: string | null, never = "(never)"): string {
  if (value === null || value === "") return never;
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return never;
  return new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(parsed);
}

/**
 * Human duration ("3m 12s"). A missing or inverted pair reads as the dash,
 * because a queued run has no span to describe yet. The largest unit
 * carries the reading and the next one qualifies it.
 */
export function formatDuration(startedAt: string | null, endedAt: string | null): string {
  const span =
    startedAt === null || endedAt === null || startedAt === "" || endedAt === ""
      ? Number.NaN
      : new Date(endedAt).getTime() - new Date(startedAt).getTime();
  if (!Number.isFinite(span) || span < 0) return "—";
  const parts = [Math.floor(span / 3_600_000), Math.floor(span / 60_000) % 60, Math.round(span / 1000) % 60];
  const [hours, minutes, seconds] = parts as [number, number, number];
  if (hours > 0) return `${hours}h ${minutes}m`;
  return minutes > 0 ? `${minutes}m ${seconds}s` : `${seconds}s`;
}

/** The one-line result cell: the failure text first, else a quiet summary. */
export function runResultExcerpt(run: Pick<AgentRun, "error" | "done_payload">): string {
  if (run.error.trim() !== "") return run.error.trim();
  const summary = run.done_payload?.["summary"];
  return typeof summary === "string" ? summary : "";
}

/** When a queued run has not started yet, its created moment stands in. */
export function runStartedLabel(run: Pick<AgentRun, "started_at" | "created_at">): string {
  if (run.started_at !== null && run.started_at !== "") return formatMoment(run.started_at);
  return `Queued ${formatMoment(run.created_at)}`;
}

/**
 * Who installed the binding, as the workspace directory describes them.
 * The actor is a user id; a member the directory no longer lists keeps the
 * raw id so the row never disappears silently. A member without a chosen
 * display name reads by their email's local part.
 */
export function installedByName(actor: string | null, directory: readonly WorkspaceMember[]): string | null {
  if (actor === null || actor === "") return null;
  const member = directory.find((row) => row.member.id === actor)?.member;
  if (member === undefined) return actor;
  if (member.display_name.trim() !== "") return member.display_name.trim();
  const email = (member.email ?? "").trim();
  const at = email.indexOf("@");
  const local = at < 0 ? email : email.slice(0, at);
  return local === "" ? actor : local;
}

export interface ConfigRow {
  label: string;
  value: string;
  /** Raw rule behind the plain-language schedule, shown on hover. */
  title?: string;
}

/**
 * The configuration grid, in the order the old surface read it. Extra and
 * skipped dates appear only when the install carries them, and the pod
 * row names the project default rather than leaving a blank.
 */
export function configRows(binding: SchedulerBindingDetail, installedBy: string | null): ConfigRow[] {
  const rows: ConfigRow[] = [
    { label: "Schedule", value: describeRrule(binding.rrule), title: binding.rrule },
    { label: "Starts at", value: formatMoment(binding.dtstart) },
    { label: "Time zone", value: displayTimeZone(binding.tzid) },
    { label: "Next run", value: formatMoment(binding.next_run_at) },
    { label: "Outcome mode", value: binding.outcome_mode },
    { label: "Pod", value: binding.pod_name ?? "(default pod)" },
  ];
  if (binding.rdates.length > 0) {
    rows.push({ label: "Extra dates", value: binding.rdates.map((date) => formatMoment(date)).join(", ") });
  }
  if (binding.exdates.length > 0) {
    rows.push({ label: "Skipped dates", value: binding.exdates.map((date) => formatMoment(date)).join(", ") });
  }
  if (installedBy !== null && installedBy !== "") {
    rows.push({ label: "Installed by", value: installedBy });
  }
  return rows;
}

/**
 * Empty-history sentence: a disabled install says why nothing will appear;
 * an enabled one names the next firing when the API knows it.
 */
export function emptyHistoryText(binding: Pick<SchedulerBindingDetail, "enabled" | "next_run_at">): string {
  if (!binding.enabled) return "Scheduler is disabled — it will not fire until re-enabled.";
  if (binding.next_run_at !== null && binding.next_run_at !== "") {
    return `No runs yet — next run at ${formatMoment(binding.next_run_at)}.`;
  }
  return "No runs yet.";
}

/**
 * Series completion (PSET-059 target, main#559). The API has not shipped
 * `series_exhausted` yet in this checkout, so an absent field reads as
 * unfinished; when it arrives the finite series badges Completed.
 */
export const SERIES_EXHAUSTED_FIELD = "series_exhausted";

/** True only when the API answered the target field with true. */
export function isSeriesCompleted(binding: object): boolean {
  return (binding as Record<string, unknown>)[SERIES_EXHAUSTED_FIELD] === true;
}

export const SERIES_COMPLETED_BADGE = "Completed";
export const SERIES_COMPLETED_TEXT = "Schedule completed — every occurrence has run.";

/** Page count for the pager, never below one. */
export function pagerTotalPages(page: { total_pages: number } | undefined): number {
  const total = page?.total_pages ?? 1;
  return total < 1 ? 1 : total;
}

/** Reading label for a run status; unknown values read as the raw code. */
const RUN_STATUS_READING: Record<string, string> = {
  queued: "Queued",
  assigned: "Assigned",
  waiting_for_worktree: "Waiting for checkout",
  running: "Running",
  cancel_requested: "Cancelling",
  awaiting_approval: "Awaiting approval",
  awaiting_reauth: "Awaiting sign-in",
  paused_awaiting_input: "Paused",
  blocked: "Blocked",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  refused: "Refused",
};

export function runStatusLabel(status: string): string {
  return RUN_STATUS_READING[status] ?? status;
}

/** Badge tone bucket for a run status: done, bad, active or quiet. */
export type RunTone = "success" | "danger" | "accent" | "neutral";

export function runStatusTone(status: string): RunTone {
  if (status === "completed") return "success";
  if (status === "failed" || status === "refused") return "danger";
  if (status === "running" || status === "assigned" || status === "waiting_for_worktree") return "accent";
  return "neutral";
}
