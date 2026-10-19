// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue activity feed (parity/specs/issues.md §16, ISS-194–196). One
// timeline over the issue's property activities and its comments, oldest
// first by default. The header narrows it by category and flips its
// order; in newest-first the comment composer sits above the timeline, in
// oldest-first below it. While the reads are in flight three skeleton
// rows stand in; an empty timeline renders no rows and no error.

import { Skeleton } from "@pidash/kit";
import type { IssueComment } from "@pidash/api-client";
import * as React from "react";

import { useIssueActivities } from "./api.js";
import { ActivityEntryRow } from "./ActivityEntryRow.js";
import { FeedHeader } from "./FeedHeader.js";
import { composeFeed, flipSort, toggleFeedFilter } from "./model.js";
import { useFeedPrefs } from "./prefs.js";

export const FEED_LOAD_FAILED = "Couldn't load the activity.";

export interface ActivityFeedProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Fetched by the thread; the feed merges it into the timeline. */
  comments: readonly IssueComment[];
  commentsPending: boolean;
  commentsError: boolean;
  /** The composer element; placed by the sort order. */
  composer: React.ReactNode;
  /** Comment card wiring, shared with the thread. */
  renderComment: (comment: IssueComment) => React.ReactNode;
  /** Archived issues keep the timeline and drop the composer. */
  archived: boolean;
}

/** Three stand-in rows while either source is still loading. */
export function FeedSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading activity" className="flex flex-col gap-(--space-4)" aria-busy="true">
      <Skeleton className="h-10 w-full" />
      <Skeleton className="h-10 w-full" />
      <Skeleton className="h-10 w-full" />
    </div>
  );
}

export function ActivityFeed({
  workspaceSlug,
  projectId,
  issueId,
  comments,
  commentsPending,
  commentsError,
  composer,
  renderComment,
  archived,
}: ActivityFeedProps): React.ReactElement {
  const activities = useIssueActivities({ workspaceSlug, projectId, issueId });
  const { filters, setFilters, order, setOrder } = useFeedPrefs();

  const pending = activities.isPending || commentsPending;
  const failed = activities.isError || commentsError;
  const entries = React.useMemo(
    () => composeFeed(activities.data ?? [], comments, filters, order),
    [activities.data, comments, filters, order]
  );

  return (
    <section aria-label="Activity">
      <FeedHeader
        filters={filters}
        onToggleFilter={(filter) => {
          setFilters(toggleFeedFilter(filters, filter));
        }}
        order={order}
        onToggleSort={() => {
          setOrder(flipSort(order));
        }}
      />
      <div className="mt-(--space-4) flex flex-col gap-(--space-4)">
        {!archived && order === "desc" ? composer : null}
        {pending ? (
          <FeedSkeleton />
        ) : failed ? (
          <p className="text-body text-(--danger)">{FEED_LOAD_FAILED}</p>
        ) : (
          <div className="flex flex-col gap-(--space-6)">
            {entries.map((entry) =>
              entry.kind === "comment" ? (
                <div key={entry.id} data-feed-row="true">
                  {renderComment(entry.comment)}
                </div>
              ) : (
                <div key={entry.id} data-feed-row="true">
                  <ActivityEntryRow workspaceSlug={workspaceSlug} activity={entry.activity} />
                </div>
              )
            )}
          </div>
        )}
        {!archived && order === "asc" ? composer : null}
      </div>
    </section>
  );
}
