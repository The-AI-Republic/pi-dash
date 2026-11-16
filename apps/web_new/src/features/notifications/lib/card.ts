// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inbox card data (written from parity/specs/notifications.md §4, §9,
// NTF-005, NTF-006, NTF-022). One resolver turns a list row into the
// card's four elements — actor, change summary, item reference with
// title, relative age — plus the read flag, and carries the archive and
// snooze timestamps the actions and the resume label need. Rows that
// cannot be resolved (missing workspace, project, item, actor or change
// description) resolve to null and the list skips them. Summary
// sentences are fresh wording over the change vocabulary; kinds outside
// it fall back to a verb-based sentence with the raw new value.
import type { NotificationItem } from "@pidash/api-client";

/** The narrowed activity payload behind one card's summary. */
export interface NotificationChange {
  field: string;
  verb: string;
  oldValue: string;
  newValue: string;
  /** Comment or description text; empty when the change carries none. */
  excerpt: string;
}

/** Everything one inbox card renders. */
export interface NotificationCardData {
  id: string;
  issueId: string;
  projectId: string;
  actorName: string;
  actorAvatarUrl: string | null;
  summary: string;
  reference: string;
  title: string;
  createdAt: string;
  unread: boolean;
  mentioned: boolean;
  triage: boolean;
  /** Archive moment; a non-null value means the card is archived. */
  archivedAt: string | null;
  /** Scheduled resume moment; a non-null value means the card is snoozed. */
  snoozedTill: string | null;
  /**
   * The row's stream annotations as the list carried them. Actions reuse
   * them when the write's echo (a bare instance) omits the flags, so the
   * settled card stays in the stream it was listed in.
   */
  streamFlags: Pick<NotificationItem, "is_inbox_issue" | "is_intake_issue" | "is_mentioned_notification">;
}

/**
 * One human-readable summary sentence for a change. Every sentence names
 * the actor first; mention rows name the viewer as the mentioned party.
 * Cases dispatch on which values are present, not on the verb: the fan-out
 * reuses "updated" for every property change and carries the added name in
 * the new value, the removed name in the old value.
 */
export function describeNotificationChange(actorName: string, change: NotificationChange, isMention: boolean): string {
  if (isMention) {
    return change.excerpt === "" ? `${actorName} mentioned you` : `${actorName} mentioned you: ${change.excerpt}`;
  }
  // Estimate fields carry the estimate type as a suffix (estimate_points,
  // estimate_hours, ...), so they match by prefix, not by name.
  if (change.field.startsWith("estimate")) {
    return `${actorName} set the estimate to ${change.newValue}`;
  }
  switch (change.field) {
    case "comment":
      return change.excerpt === "" ? `${actorName} commented` : `${actorName} commented: ${change.excerpt}`;
    case "description":
      return change.excerpt === ""
        ? `${actorName} edited the description`
        : `${actorName} edited the description: ${change.excerpt}`;
    case "state":
      return `${actorName} set the state to ${change.newValue}`;
    case "assignees":
    case "assignee":
      if (change.newValue !== "") return `${actorName} assigned ${change.newValue}`;
      if (change.oldValue !== "") return `${actorName} unassigned ${change.oldValue}`;
      return `${actorName} changed the assignees`;
    case "start_date":
      return change.newValue === ""
        ? `${actorName} removed the start date`
        : `${actorName} changed the start date to ${change.newValue}`;
    case "target_date":
      return change.newValue === ""
        ? `${actorName} removed the due date`
        : `${actorName} changed the due date to ${change.newValue}`;
    case "labels":
      if (change.newValue !== "") return `${actorName} added the ${change.newValue} label`;
      if (change.oldValue !== "") return `${actorName} removed the ${change.oldValue} label`;
      return `${actorName} changed the labels`;
    case "parent":
      return change.newValue === ""
        ? `${actorName} removed the parent`
        : `${actorName} set the parent to ${change.newValue}`;
    case "link":
    case "blocks":
      if (change.newValue !== "") return `${actorName} linked ${change.newValue}`;
      if (change.oldValue !== "") return `${actorName} unlinked ${change.oldValue}`;
      return `${actorName} changed the relations`;
    case "attachment":
      if (change.verb === "created") return `${actorName} attached a file`;
      if (change.verb === "deleted" || change.verb === "removed") return `${actorName} removed an attachment`;
      return `${actorName} changed the attachments`;
    case "archived_at":
      return change.newValue === "restore" ? `${actorName} restored this issue` : `${actorName} archived this issue`;
    default:
      return change.newValue === ""
        ? `${actorName} ${change.verb} ${change.field}`
        : `${actorName} ${change.verb} ${change.field}: ${change.newValue}`;
  }
}

/** Plain text from an HTML fragment, for description excerpts. */
function stripMarkup(html: string): string {
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function textField(record: Record<string, unknown>, name: string): string {
  const value = record[name];
  return typeof value === "string" ? value : "";
}

/**
 * Resolve one list row to its card, or null when the row cannot be
 * resolved. Resolution never throws: unexpected payload shapes skip.
 */
export function resolveNotificationCard(row: NotificationItem): NotificationCardData | null {
  if (row.workspace === "") return null;
  if (row.project === null) return null;
  const actor = row.triggered_by_details;
  if (actor === null) return null;
  if (!isRecord(row.data)) return null;
  const issue = row.data["issue"];
  if (!isRecord(issue)) return null;
  const issueId = textField(issue, "id");
  if (issueId === "") return null;
  const title = textField(issue, "name");
  if (title === "") return null;
  const identifier = textField(issue, "identifier");
  if (identifier === "") return null;
  const sequence = issue["sequence_id"];
  if (typeof sequence !== "number") return null;
  const activity = row.data["issue_activity"];
  if (!isRecord(activity)) return null;
  const field = textField(activity, "field");
  if (field === "") return null;
  const verb = textField(activity, "verb");
  if (verb === "") return null;
  const oldValue = textField(activity, "old_value");
  const newValue = textField(activity, "new_value");
  const excerpt = field === "description" ? stripMarkup(newValue) : textField(activity, "issue_comment").trim();
  return {
    id: row.id,
    issueId,
    projectId: row.project,
    actorName: actor.display_name,
    actorAvatarUrl: actor.avatar_url,
    summary: describeNotificationChange(
      actor.display_name,
      { field, verb, oldValue, newValue, excerpt },
      row.is_mentioned_notification === true
    ),
    reference: `${identifier}-${sequence}`,
    title,
    createdAt: row.created_at,
    unread: row.read_at === null,
    mentioned: row.is_mentioned_notification === true,
    triage: row.is_inbox_issue === true || row.is_intake_issue === true,
    archivedAt: row.archived_at,
    snoozedTill: row.snoozed_till,
    streamFlags: {
      is_inbox_issue: row.is_inbox_issue,
      is_intake_issue: row.is_intake_issue,
      is_mentioned_notification: row.is_mentioned_notification,
    },
  };
}

/**
 * Resolve a stream page to its cards in server order (newest first),
 * skipping unresolvable rows.
 */
export function resolveNotificationCards(rows: NotificationItem[]): NotificationCardData[] {
  const cards: NotificationCardData[] = [];
  for (const row of rows) {
    const card = resolveNotificationCard(row);
    if (card !== null) cards.push(card);
  }
  return cards;
}
