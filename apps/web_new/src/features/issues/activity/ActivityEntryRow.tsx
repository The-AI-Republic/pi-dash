// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One property-change row of the feed (parity/specs/issues.md §16,
// ISS-194): the type badge on the rail, the actor, what changed, and how
// long ago, with the exact timestamp in the tooltip. The actor links to
// their profile, so the first entry's link is the feed's way into the
// member surface. System rows keep the product name in the text but hang a
// type badge instead of an avatar, so they never read as member comments.

import { Tooltip } from "@pidash/kit";
import { isEstimateActivityField, type IssueActivity } from "@pidash/api-client";
import {
  AlarmClock,
  Archive,
  AlignLeft,
  Ban,
  CalendarDays,
  CircleDot,
  Clock,
  Copy,
  Hash,
  Hourglass,
  Inbox,
  Link,
  Link2,
  ListTree,
  Network,
  Package,
  Paperclip,
  Plus,
  Repeat,
  RotateCcw,
  ShieldAlert,
  Signal,
  Tag,
  Trash2,
  Type,
  Users,
  type LucideIcon,
} from "lucide-react";
import * as React from "react";

import { timeAgo } from "../../../shared/lib/timeAgo.js";
import { activityActorName, activitySentence } from "./model.js";
import { TimelineRow, type TimelinePosition } from "./TimelineRow.js";

export interface ActivityEntryRowProps {
  workspaceSlug: string;
  activity: IssueActivity;
  /** Where the row sits in the timeline; trims the rail at the ends. */
  position?: TimelinePosition | undefined;
}

interface ActivityBadge {
  icon: LucideIcon;
  label: string;
}

/**
 * Badge glyph per activity field; relations get one glyph per relation
 * type and the archive row swaps to the restore glyph on a restore.
 */
export function activityBadgeFor(activity: Pick<IssueActivity, "field" | "verb" | "new_value">): ActivityBadge {
  if (activity.field === null) {
    return activity.verb === "deleted" ? { icon: Trash2, label: "Deletion" } : { icon: Plus, label: "Creation" };
  }
  if (isEstimateActivityField(activity.field)) return { icon: Hash, label: "Estimate" };
  switch (activity.field) {
    case "name":
      return { icon: Type, label: "Name" };
    case "description":
      return { icon: AlignLeft, label: "Description" };
    case "state":
      return { icon: CircleDot, label: "State" };
    case "assignees":
      return { icon: Users, label: "Assignees" };
    case "priority":
      return { icon: Signal, label: "Priority" };
    case "labels":
      return { icon: Tag, label: "Labels" };
    case "parent":
      return { icon: ListTree, label: "Parent" };
    case "blocking":
      return { icon: Ban, label: "Blocking" };
    case "blocked_by":
      return { icon: ShieldAlert, label: "Blocked by" };
    case "duplicate":
      return { icon: Copy, label: "Duplicate" };
    case "relates_to":
      return { icon: Link2, label: "Related" };
    case "start_date":
      return { icon: CalendarDays, label: "Start date" };
    case "target_date":
      return { icon: CalendarDays, label: "Due date" };
    case "cycles":
      return { icon: Repeat, label: "Cycle" };
    case "modules":
      return { icon: Package, label: "Module" };
    case "link":
      return { icon: Link, label: "Link" };
    case "attachment":
      return { icon: Paperclip, label: "Attachment" };
    case "archived_at":
      return activity.new_value === "restore"
        ? { icon: RotateCcw, label: "Restored" }
        : { icon: Archive, label: "Archived" };
    case "agent_wait":
      return { icon: Hourglass, label: "Agent wait" };
    case "agent_wake":
      return { icon: AlarmClock, label: "Agent wake" };
    case "intake":
    case "inbox":
      return { icon: Inbox, label: "Intake" };
    case "worklog":
      return { icon: Clock, label: "Worklog" };
    case "issue":
      return { icon: Trash2, label: "Deletion" };
    default:
      return { icon: Network, label: "Activity" };
  }
}

export function ActivityEntryRow({
  workspaceSlug,
  activity,
  position = "only",
}: ActivityEntryRowProps): React.ReactElement | null {
  const sentence = activitySentence(activity);
  if (sentence === null) return null;
  const name = activityActorName(activity);
  const actorId = activity.actor_detail?.id ?? activity.actor;
  // Product-authored rows (waits, wakes, archives) credit Pi Dash rather
  // than a member, so there is no profile to link to.
  const member = activity.actor_detail !== null && name === activity.actor_detail.display_name;
  const { icon: Icon, label } = activityBadgeFor(activity);

  return (
    <TimelineRow position={position} badgeLabel={label} badge={<Icon size={14} strokeWidth={1.5} aria-hidden="true" />}>
      <p className="text-meta flex min-h-7 items-center leading-relaxed text-(--text-muted)">
        {!member ? (
          <span className="font-medium text-(--text)">{name}</span>
        ) : (
          // The member profile is a shell route; a plain address keeps
          // this row independent of its loader.
          <a href={`/${workspaceSlug}/profile/${actorId}`} className="font-medium text-(--text) hover:underline">
            {name}
          </a>
        )}{" "}
        <span>{sentence}</span>{" "}
        <Tooltip content={new Date(activity.created_at).toLocaleString()}>
          <span className="whitespace-nowrap text-(--text-muted)">{timeAgo(activity.created_at)}</span>
        </Tooltip>
      </p>
    </TimelineRow>
  );
}
