// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member-timezone date range on the cycle detail page (PROF-017
// stored-dates fix, NEWFRONT-859). Renders the cycle's stored start/end
// dates in the signed-in member's timezone so the profile parity
// scenario can read them; the cycles detail header and sidebar (slices
// F/G) own the richer CYC-044 presentation and may adopt or replace
// this display when they land.

import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { useCycleDetail } from "../api/cycleDetail.js";
import { formatMemberDate } from "../lib/memberDates.js";

/** Visible label for the member-timezone date range. */
export const CYCLE_MEMBER_DATES_LABEL = "Dates in your timezone";

export interface CycleMemberDateRange {
  startDate: string | null;
  endDate: string | null;
}

/**
 * The date range from plain props: nothing renders without a stored
 * date, a lone date renders one-sided, and a pair renders joined. Pure
 * for tests; the connected component below feeds it from queries.
 */
export function CycleMemberDatesFrame({
  range,
  timeZone,
}: {
  range: CycleMemberDateRange;
  timeZone: string | null | undefined;
}): React.ReactElement | null {
  const { startDate, endDate } = range;
  if (startDate === null && endDate === null) return null;
  return (
    <section
      data-testid="cycle-member-dates"
      aria-label={CYCLE_MEMBER_DATES_LABEL}
      className="text-body flex flex-wrap items-center gap-(--space-2) text-(--text)"
    >
      <span>{CYCLE_MEMBER_DATES_LABEL}</span>
      {startDate === null ? null : (
        <span data-testid="cycle-member-start-date">{formatMemberDate(startDate, timeZone)}</span>
      )}
      {startDate !== null && endDate !== null ? <span aria-hidden="true">–</span> : null}
      {endDate === null ? null : <span data-testid="cycle-member-end-date">{formatMemberDate(endDate, timeZone)}</span>}
    </section>
  );
}

/** The range for a cycle, read from the detail and session caches. */
export function CycleMemberDates({
  workspaceSlug,
  projectId,
  cycleId,
}: {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
}): React.ReactElement | null {
  const cycle = useCycleDetail(workspaceSlug, projectId, cycleId);
  const me = useMe();
  return (
    <CycleMemberDatesFrame
      range={{ startDate: cycle.data?.start_date ?? null, endDate: cycle.data?.end_date ?? null }}
      timeZone={me.data?.user_timezone}
    />
  );
}
