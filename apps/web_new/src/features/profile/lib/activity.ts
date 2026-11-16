// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity row presentation (parity/specs/profile.md §1 and §3, PROF-005
// and PROF-007). One feed serves the summary's recent list and the full
// activity tab. Each row names its actor, paraphrases the change into a
// sentence, carries the work item it touched and links into the browse
// route by its short key. Rows whose work item was not serialised still
// render their sentence without a link.

import type { IssueActivity } from "@pidash/api-client";

/** Human names for the fields an activity row can change. */
const FIELD_NAMES: Record<string, string> = {
  name: "the title",
  description: "the description",
  state: "the state",
  priority: "the priority",
  assignees: "the assignees",
  labels: "the labels",
  start_date: "the start date",
  target_date: "the target date",
  estimate_point: "the estimate",
  cycle: "the cycle",
  module: "the module",
  parent: "the parent",
  archived_at: "the archive",
  attachment: "an attachment",
  link: "a link",
  comment: "a comment",
};

/** The work item a row points at: short key plus browse address. */
export interface ActivityIssueRef {
  key: string;
  href: string;
}

/**
 * Browse reference for a row's work item. The feed serialises the flat
 * issue (sequence id) and the project (identifier); a row missing either
 * renders without a link.
 */
export function activityIssueRef(
  row: Pick<IssueActivity, "issue_detail" | "project_detail">,
  workspaceSlug: string
): ActivityIssueRef | null {
  const issue = row.issue_detail ?? null;
  const project = row.project_detail ?? null;
  if (issue === null || project === null || project.identifier === "") return null;
  const key = `${project.identifier}-${issue.sequence_id}`;
  return { key, href: `/${workspaceSlug}/browse/${key}` };
}

/** The work item name a row touched, or null when it was not serialised. */
export function activityIssueName(row: Pick<IssueActivity, "issue_detail">): string | null {
  const issue = row.issue_detail ?? null;
  return issue === null ? null : issue.name;
}

/** Actor name shown at the head of the row; the signed-in member reads "You". */
export function activityActorName(row: Pick<IssueActivity, "actor_detail">, currentUserId: string | null): string {
  const actor = row.actor_detail;
  if (actor === null) return "Someone";
  if (currentUserId !== null && actor.id === currentUserId) return "You";
  if (actor.display_name.trim() !== "") return actor.display_name;
  return `${actor.first_name} ${actor.last_name}`.trim() || "Someone";
}

/** Sentence fragment describing what the actor did. */
export function activitySummary(row: Pick<IssueActivity, "field" | "old_value" | "new_value">): string {
  const field = row.field === null ? null : (FIELD_NAMES[row.field] ?? row.field.replace(/_/g, " "));
  if (field === "the archive") {
    return row.new_value === "restore" ? "restored the work item" : "archived the work item";
  }
  if (field === "a comment") return "commented on the work item";
  if (field === null) return "updated the work item";
  if (row.old_value === null || row.old_value === "") return `added ${field}`;
  if (row.new_value === null || row.new_value === "") return `removed ${field}`;
  return `changed ${field}`;
}

/** Merge appended pages without repeating a row across the boundary. */
export function appendActivityPage(rows: readonly IssueActivity[], page: readonly IssueActivity[]): IssueActivity[] {
  const seen = new Set(rows.map((row) => row.id));
  return [...rows, ...page.filter((row) => !seen.has(row.id))];
}
