// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reader-facing text for the firing calendar and its panel
// (parity/specs/agents.md §5, AGT-015/017). A firing is described in the
// zone its install stores, so the time shown is the time the schedule
// means, not the viewer's own clock.

import type { SchedulerBinding, SchedulerOccurrence } from "@pidash/api-client";

import { describeStoredRrule } from "./schedule.js";

function instantFormatFor(tzid: string): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
    timeZoneName: "longOffset",
    timeZone: tzid,
  });
}

/**
 * One firing's instant in the install's zone, offset included. Falls back
 * to the ISO instant when the zone name is unknown to the runtime, so the
 * read stays unambiguous rather than silently borrowing the viewer's clock.
 */
export function formatOccurrenceInstant(dtstart: string, tzid: string): string {
  const at = new Date(dtstart);
  if (Number.isNaN(at.getTime())) return "";
  try {
    return instantFormatFor(tzid)
      .formatToParts(at)
      .map((part) => (part.type === "timeZoneName" ? part.value.replace(/^GMT([+-]00:00)?$/, "UTC") : part.value))
      .join("");
  } catch {
    // An IANA name this runtime does not know: show the unambiguous instant.
    return at.toISOString();
  }
}

/** The panel's status line: a future firing is Scheduled, a past one reports its run. */
export function occurrenceStatus(occurrence: Pick<SchedulerOccurrence, "kind" | "status">): string {
  if (occurrence.kind === "scheduled") return "Scheduled";
  const status = occurrence.status?.trim();
  return status === undefined || status === "" ? "Past" : status;
}

/** Plain-language repetition of the install's rule ("every day"). */
export function occurrenceRecurrence(binding: Pick<SchedulerBinding, "rrule"> | null): string {
  return binding === null ? "" : describeStoredRrule(binding.rrule);
}

/** Project context carried by the install, or the quiet placeholder. */
export function occurrenceContext(binding: Pick<SchedulerBinding, "extra_context"> | null): string {
  const context = binding?.extra_context.trim() ?? "";
  return context === "" ? "No extra context" : context;
}

/** Past firings link to their run; the calendar never exports them. */
export function occurrenceRunHref(
  workspaceSlug: string,
  projectId: string,
  occurrence: Pick<SchedulerOccurrence, "agent_run_id">
): string | null {
  if (occurrence.agent_run_id === null) return null;
  return `/${workspaceSlug}/projects/${projectId}/runners/detail/${occurrence.agent_run_id}`;
}
