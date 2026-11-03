// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud transfer-hop note body (parity/specs/issues.md §20/§23, ISS-236):
// names the cycle the item moved from and when. The server records every
// cycle move on the issue history (field "cycles", old/new names plus
// their identifiers), so the note reads the same history the feed shows
// — no cloud-only endpoint needed. An item added to its cycle directly
// (no previous cycle) shows nothing: that is an add, not a hop. Loaded
// as its own chunk behind the provider, so the community bundle never
// carries the history read.
import * as React from "react";

import type { IssueActivity } from "@pidash/api-client";
import { timeAgo } from "../../../shared/lib/timeAgo.js";
import { useIssueActivities } from "../activity/api.js";
import type { DetailBadgeInput } from "./providers.js";

/** One cycle-to-cycle hop: where the item came from and when it moved. */
export interface TransferHop {
  fromName: string;
  movedAt: string;
}

/** The history field the server stamps on cycle moves. */
export const CYCLE_ACTIVITY_FIELD = "cycles";

/**
 * The latest hop into `cycleId`, or null when the item reached it without
 * a previous cycle (added directly, or the stored name is blank) or never
 * moved at all. History rows arrive oldest-first, so the walk runs from
 * the end and the first row into this cycle decides.
 */
export function selectTransferHop(rows: readonly IssueActivity[], cycleId: string): TransferHop | null {
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const row = rows[index]!;
    if (row.field !== CYCLE_ACTIVITY_FIELD || row.new_identifier !== cycleId) continue;
    const fromName = row.old_value;
    if (row.old_identifier === null || fromName === null || fromName === "") return null;
    return { fromName, movedAt: row.created_at };
  }
  return null;
}

/** The note's sentence for a hop. */
export function transferHopLabel(hop: TransferHop): string {
  return `Moved from ${hop.fromName}, ${timeAgo(hop.movedAt)}`;
}

export function TransferHopNoteContent(input: DetailBadgeInput): React.ReactElement | null {
  const activities = useIssueActivities({
    workspaceSlug: input.workspaceSlug,
    projectId: input.projectId,
    issueId: input.issueId,
  });
  const hop = React.useMemo(
    () => (input.cycleId === null ? null : selectTransferHop(activities.data ?? [], input.cycleId)),
    [activities.data, input.cycleId]
  );
  if (hop === null) return null;
  return <span className="text-meta text-(--text-muted)">{transferHopLabel(hop)}</span>;
}
