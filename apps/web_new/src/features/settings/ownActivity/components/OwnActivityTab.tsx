// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history tab (parity/specs/profile.md §7, PROF-019). Lists the
// signed-in member's changes across every workspace, newest first, one
// hundred rows per page. "Load more" shows only while the server says a
// further page exists; a member with no history sees an illustration
// that follows the light or dark mode currently applied.

import { Button, EmptyState, Spinner } from "@pidash/kit";
import * as React from "react";

import { useMe } from "../../../../core/session/index.js";
import { useResolvedScheme } from "../../../../core/theme/index.js";
import { CrossAppLink } from "../../../../shared/coexistence/index.js";
import { useOwnActivityPages } from "../api.js";
import { activityAge } from "../lib/relativeTime.js";
import { ownActivityRows } from "../lib/rows.js";

export const ACTIVITY_HEADING = "Activity";
export const ACTIVITY_INTRO = "Everything you changed, across every workspace you belong to.";
export const ACTIVITY_LOADING_LABEL = "Loading your activity";
export const ACTIVITY_LOAD_FAILED = "Couldn't load your activity. Try again in a moment.";
export const ACTIVITY_LOAD_MORE_LABEL = "Load more";
export const ACTIVITY_EMPTY_TITLE = "No activity yet";
export const ACTIVITY_EMPTY_DESCRIPTION = "Once you create, comment on or move work items, your changes show up here.";

/** Art scheme: any dark document mark selects the dark illustration. */
export function activityArtScheme(scheme: string): "light" | "dark" {
  return scheme === "light" || scheme === "light-contrast" ? "light" : "dark";
}

export interface OwnActivityTabProps {
  /** Injected clock so tests and stories pin the relative ages. */
  now?: Date | undefined;
}

export function OwnActivityTab({ now = new Date() }: OwnActivityTabProps): React.ReactElement {
  const pages = useOwnActivityPages();
  const me = useMe();
  const { scheme } = useResolvedScheme();
  const rows = React.useMemo(() => ownActivityRows(pages.data?.pages ?? []), [pages.data]);
  const timezone = me.data?.user_timezone;

  return (
    <section aria-labelledby="own-activity-heading" className="flex flex-col gap-(--space-6)">
      <header className="flex flex-col gap-(--space-2)">
        <h1 id="own-activity-heading" className="text-h2 text-(--text)">
          {ACTIVITY_HEADING}
        </h1>
        <p className="text-body text-(--text-muted)">{ACTIVITY_INTRO}</p>
      </header>

      {pages.isPending ? (
        <div data-testid="own-activity-loading" className="grid place-items-center py-(--space-8)">
          <Spinner label={ACTIVITY_LOADING_LABEL} />
        </div>
      ) : pages.isError ? (
        <p role="alert" className="text-body text-(--text)">
          {ACTIVITY_LOAD_FAILED}
        </p>
      ) : rows.length === 0 ? (
        <EmptyState
          variant="illustrated"
          art={{ category: "activity", type: "activity" }}
          scheme={activityArtScheme(scheme)}
          title={ACTIVITY_EMPTY_TITLE}
          description={ACTIVITY_EMPTY_DESCRIPTION}
        />
      ) : (
        <div className="flex flex-col gap-(--space-4)">
          <ul data-testid="settings-activity-list" className="flex flex-col divide-y divide-(--border)">
            {rows.map((row) => {
              const body = (
                <>
                  <div className="flex flex-wrap items-baseline gap-(--space-2)">
                    {row.reference === null ? null : (
                      <span className="text-meta text-(--text-muted) tabular-nums">{row.reference}</span>
                    )}
                    <span className="text-body font-medium text-(--text)">{row.title}</span>
                  </div>
                  <p className="text-body text-(--text-muted)">{row.summary}</p>
                  <div className="text-meta flex flex-wrap items-center gap-(--space-2) text-(--text-muted)">
                    {row.workspace === null ? null : <span>{row.workspace.name}</span>}
                    <time dateTime={row.createdAt}>{activityAge(row.createdAt, now.getTime(), timezone)}</time>
                  </div>
                </>
              );
              return (
                <li key={row.id} data-testid="own-activity-row" className="py-(--space-3)">
                  {row.href === null ? (
                    <div className="flex flex-col gap-(--space-1)">{body}</div>
                  ) : (
                    <CrossAppLink
                      to={row.href}
                      className="flex flex-col gap-(--space-1) rounded-(--radius-control) text-(--text) hover:bg-(--surface-hover) focus-visible:outline-2 focus-visible:outline-(--accent)"
                    >
                      {body}
                    </CrossAppLink>
                  )}
                </li>
              );
            })}
          </ul>
          {pages.hasNextPage ? (
            <div>
              <Button
                type="button"
                variant="secondary"
                loading={pages.isFetchingNextPage}
                onClick={() => void pages.fetchNextPage()}
              >
                {ACTIVITY_LOAD_MORE_LABEL}
              </Button>
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}
