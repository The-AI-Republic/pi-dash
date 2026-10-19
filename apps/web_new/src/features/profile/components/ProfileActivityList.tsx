// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Activity rows and the full activity tab (parity/specs/profile.md §3,
// PROF-007). One row renderer serves the summary's recent list and the
// tab: actor, sentence, work-item link and time. The tab pages one
// hundred rows at a time and appends further pages through an explicit
// load-more control that retires on the last page; a quiet member sees
// neither rows nor the control. The download control lives in
// ProfileActivityDownload and is gated by the caller.

import { getUserProfileActivity, type IssueActivity, type UserActivityPage } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { timeAgo } from "../../../shared/lib/timeAgo.js";
import {
  activityActorName,
  activityIssueName,
  activityIssueRef,
  activitySummary,
  appendActivityPage,
} from "../lib/activity.js";
import { PROFILE_ACTIVITY_PAGE_SIZE, profileActivityQueryOptions } from "../api/queries.js";

export interface ActivityRowProps {
  row: IssueActivity;
  workspaceSlug: string;
  currentUserId: string | null;
}

export function ActivityRow({ row, workspaceSlug, currentUserId }: ActivityRowProps): React.ReactElement {
  const ref = activityIssueRef(row, workspaceSlug);
  const name = activityIssueName(row);
  return (
    <li data-profile-activity-row={row.id} className="flex items-start gap-(--space-3) px-(--space-3) py-(--space-2)">
      <span aria-hidden="true" className="mt-(--space-1) size-2 shrink-0 rounded-(--radius-full) bg-(--accent)" />
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-1)">
        <p className="text-body min-w-0 break-words text-(--text-muted)">
          <span className="font-medium text-(--text)">{activityActorName(row, currentUserId)}</span>{" "}
          {activitySummary(row)}
          {name === null ? null : (
            <>
              {" "}
              <span className="text-(--text)">{name}</span>
            </>
          )}
          {ref === null ? null : (
            <>
              {" "}
              <Link
                to={ref.href}
                data-profile-activity-link={row.id}
                className="font-[family-name:var(--font-mono)] text-(--accent) hover:underline"
              >
                {ref.key}
              </Link>
            </>
          )}
        </p>
        <p className="text-caption text-(--text-muted)">{timeAgo(row.created_at)}</p>
      </div>
    </li>
  );
}

export interface ProfileActivityTabProps {
  workspaceSlug: string;
  userId: string;
  currentUserId: string | null;
  download?: React.ReactNode;
  /** Injected for tests; defaults to the live first page. */
  initialPage?: UserActivityPage;
}

export function ProfileActivityTab({
  workspaceSlug,
  userId,
  currentUserId,
  download,
  initialPage,
}: ProfileActivityTabProps): React.ReactElement {
  const client = getAppClient();
  const first = useQuery({
    ...profileActivityQueryOptions(client, workspaceSlug, userId, PROFILE_ACTIVITY_PAGE_SIZE),
    ...(initialPage === undefined ? {} : { initialData: initialPage }),
  });
  const [extra, setExtra] = React.useState<IssueActivity[]>([]);
  const [cursor, setCursor] = React.useState<string | null>(
    initialPage?.next_page_results === true ? initialPage.next_cursor : null
  );
  const [loading, setLoading] = React.useState(false);

  React.useEffect(() => {
    const page = first.data;
    if (page === undefined) return;
    setExtra([]);
    setCursor(page.next_page_results ? page.next_cursor : null);
  }, [first.data]);

  const rows = React.useMemo(() => appendActivityPage(first.data?.results ?? [], extra), [first.data, extra]);

  const loadMore = async (): Promise<void> => {
    if (cursor === null || loading) return;
    setLoading(true);
    try {
      const page = await getUserProfileActivity(client, workspaceSlug, userId, {
        perPage: PROFILE_ACTIVITY_PAGE_SIZE,
        cursor,
      });
      setExtra((current) => appendActivityPage(current, page.results));
      setCursor(page.next_page_results ? page.next_cursor : null);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div data-testid="profile-activity" className="flex flex-col">
      {download === undefined ? null : (
        <div className="flex items-center justify-end px-(--space-3) py-(--space-2)">{download}</div>
      )}
      <ul
        aria-label="Activity"
        data-testid="profile-activity-list"
        className="flex flex-col divide-y divide-(--border)"
      >
        {rows.map((row) => (
          <ActivityRow key={row.id} row={row} workspaceSlug={workspaceSlug} currentUserId={currentUserId} />
        ))}
      </ul>
      {cursor === null ? null : (
        <div className="flex justify-center p-(--space-3)">
          <Button
            variant="secondary"
            size="small"
            data-testid="profile-activity-load-more"
            disabled={loading}
            onClick={() => {
              void loadMore();
            }}
          >
            {loading ? "Loading…" : "Load more"}
          </Button>
        </div>
      )}
    </div>
  );
}
