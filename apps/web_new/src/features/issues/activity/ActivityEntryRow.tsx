// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One property-change row of the feed (parity/specs/issues.md §16,
// ISS-194): the actor, what changed, and how long ago, with the exact
// timestamp in the tooltip. The actor links to their profile, so the
// first entry's link is the feed's way into the member surface.

import { Avatar, Tooltip } from "@pidash/kit";
import type { IssueActivity } from "@pidash/api-client";
import * as React from "react";

import { timeAgo } from "../../../shared/lib/timeAgo.js";
import { activityActorName, activitySentence } from "./model.js";

export interface ActivityEntryRowProps {
  workspaceSlug: string;
  activity: IssueActivity;
}

export function ActivityEntryRow({ workspaceSlug, activity }: ActivityEntryRowProps): React.ReactElement | null {
  const sentence = activitySentence(activity);
  if (sentence === null) return null;
  const name = activityActorName(activity);
  const avatarUrl = activity.actor_detail?.avatar_url ?? undefined;
  const actorId = activity.actor_detail?.id ?? activity.actor;
  // Product-authored rows (waits, wakes, archives) credit Pi Dash rather
  // than a member, so there is no profile to link to.
  const member = activity.actor_detail !== null && name === activity.actor_detail.display_name;

  return (
    <div className="text-meta flex items-start gap-(--space-2) text-(--text-muted)">
      <Avatar name={name} {...(avatarUrl === undefined ? {} : { src: avatarUrl })} size="small" className="mt-0.5" />
      <p className="min-w-0 flex-1 leading-relaxed">
        {actorId === null || !member ? (
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
    </div>
  );
}
