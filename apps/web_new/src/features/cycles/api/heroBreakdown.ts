// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Hero breakdown clicks (parity/specs/cycles.md §15, CYC-041). A
// progress-group or breakdown entry toggles its value inside the
// cycle's stored rich filters and opens the filtered detail (priority
// entries also peek the picked work item). The click reads the
// preferences first — the read creates the row when none exists, so a
// first click works before any user-properties row does (the
// NEWFRONT-266 intended behavior) — then writes the merged filters,
// refreshes the cached copy the detail reads, and navigates. A
// refused write reports and stays on the list.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { loadRichFilterBuilder, prefsQueryOptions, writeEntityPrefs, type IssuesEntity } from "../../issues/index.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { toggleBreakdownFilter, type BreakdownDimension } from "../lib/cycleBreakdownFilter.js";
import { cycleDetailPath } from "../lib/cycleScreen.js";

/** A refused filter write reports and stays on the list. */
export const HERO_FILTER_FAILED_TITLE = "Couldn't apply the filter. Try again.";

export interface HeroBreakdownTarget {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
}

export function useApplyBreakdownFilter(target: HeroBreakdownTarget): {
  /** Toggle the value in its dimension, persist, and open the detail. */
  apply: (dimension: BreakdownDimension, value: string, peekIssueId?: string) => Promise<void>;
  pending: boolean;
} {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [pending, setPending] = React.useState(false);
  const flying = React.useRef(false);
  const entity: IssuesEntity = React.useMemo(
    () => ({ scope: "cycle", projectId: target.projectId, cycleId: target.cycleId }),
    [target.projectId, target.cycleId]
  );

  const apply = React.useCallback(
    async (dimension: BreakdownDimension, value: string, peekIssueId?: string) => {
      // A ref, not the pending state: synchronous double-clicks share
      // one render and must still collapse into one write.
      if (flying.current) return;
      flying.current = true;
      setPending(true);
      try {
        const client = getAppClient();
        const options = prefsQueryOptions(client, target.workspaceSlug, entity);
        const record = await queryClient.fetchQuery(options);
        const { parseRichExpression, serializeRichExpression } = await loadRichFilterBuilder();
        const next = await writeEntityPrefs(client, target.workspaceSlug, entity, {
          rich_filters: serializeRichExpression(
            toggleBreakdownFilter(parseRichExpression(record.rich_filters), dimension, value)
          ),
        });
        // The detail reads the cached copy with an infinite stale time;
        // the write's answer refreshes it so the filter shows at once.
        queryClient.setQueryData(options.queryKey, next);
        const detail = cycleDetailPath(target.workspaceSlug, target.projectId, target.cycleId);
        await navigate({
          to: peekIssueId === undefined ? detail : `${detail}?peekIssueId=${encodeURIComponent(peekIssueId)}`,
        });
      } catch {
        notifyError(HERO_FILTER_FAILED_TITLE);
      } finally {
        flying.current = false;
        setPending(false);
      }
    },
    [entity, navigate, queryClient, target.cycleId, target.projectId, target.workspaceSlug]
  );

  return { apply, pending };
}
