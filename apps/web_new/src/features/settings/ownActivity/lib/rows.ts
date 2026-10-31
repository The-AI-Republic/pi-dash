// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history row model (parity/specs/profile.md §7, PROF-019). The feed
// returns property-change rows across every workspace the caller belongs
// to; each row resolves to one sentence naming the item it touched, the
// item's reference, the workspace it lives in and the moment it happened.
// Rows the server no longer can name (a deleted item, a deleted project)
// still render: they keep the change verb and drop the link.

import type { IssueActivity } from "@pidash/api-client";

/** One rendered row of the own-activity feed. */
export interface OwnActivityRow {
  id: string;
  /** Reference like "ACME-12", or null when the item is gone. */
  reference: string | null;
  /** Item title, or the fallback the row shows for a missing item. */
  title: string;
  /** Workspace slug and name the row happened in, or null when unknown. */
  workspace: { slug: string; name: string } | null;
  /** What changed, in one sentence. */
  summary: string;
  /** ISO moment the change was recorded. */
  createdAt: string;
  /** Address of the item when it still resolves. */
  href: string | null;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** Trim an HTML fragment to its text for a one-line excerpt. */
function plainText(html: string): string {
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export const MISSING_ITEM_TITLE = "A deleted work item";

/**
 * One sentence for the change. Comments and descriptions carry their
 * text as an excerpt; property changes name the field and its new value.
 */
export function describeOwnActivity(activity: IssueActivity): string {
  const field = activity.field ?? "";
  const verb = activity.verb;
  const newValue = text(activity.new_value);
  const oldValue = text(activity.old_value);
  if (field === "comment") {
    // The comment's text rides in the value fields; `issue_comment` is
    // only the comment's id and names nothing.
    const excerpt = plainText(newValue === "" ? oldValue : newValue);
    return excerpt === "" ? "Commented" : `Commented: ${excerpt}`;
  }
  if (field === "description") {
    const excerpt = plainText(newValue);
    return excerpt === "" ? "Edited the description" : `Edited the description: ${excerpt}`;
  }
  if (field === "state") return newValue === "" ? "Changed the state" : `Moved the state to ${newValue}`;
  if (field === "assignees" || field === "assignee") {
    if (newValue !== "") return `Added an assignee: ${newValue}`;
    if (oldValue !== "") return `Removed an assignee: ${oldValue}`;
    return "Changed the assignees";
  }
  if (field === "archived_at") return newValue === "restore" ? "Restored the item" : "Archived the item";
  if (field !== "") {
    return newValue === ""
      ? `Cleared the ${field.replaceAll("_", " ")}`
      : `Changed the ${field.replaceAll("_", " ")} to ${newValue}`;
  }
  return verb === "" ? "Changed an item" : `Updated an item (${verb})`;
}

/** Resolve one server row to the row the tab renders. */
export function ownActivityRow(activity: IssueActivity): OwnActivityRow {
  const issue = activity.issue_detail;
  const project = activity.project_detail;
  const workspace = activity.workspace_detail;
  const sequence = issue?.sequence_id;
  const reference =
    issue !== null && project !== null && typeof sequence === "number" ? `${project.identifier}-${sequence}` : null;
  return {
    id: activity.id,
    reference,
    title: issue?.name ?? MISSING_ITEM_TITLE,
    workspace: workspace === null || workspace === undefined ? null : { slug: workspace.slug, name: workspace.name },
    summary: describeOwnActivity(activity),
    createdAt: activity.created_at,
    href: reference !== null && workspace !== null && issue !== null ? `/${workspace.slug}/browse/${reference}` : null,
  };
}

/** Every page's rows flattened in server order. */
export function ownActivityRows(pages: ReadonlyArray<{ results: IssueActivity[] }>): OwnActivityRow[] {
  return pages.flatMap((page) => page.results.map(ownActivityRow));
}
