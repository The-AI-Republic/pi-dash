/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import { useParams, useSearchParams } from "react-router";
import { EIssueServiceType, EIssuesStoreType } from "@pi-dash/types";
// hooks
import { useIssueDetail } from "@/hooks/store/use-issue-detail";
import { useIssueStoreType } from "@/hooks/use-issue-layout-store";

const PEEK_ISSUE_QUERY_KEY = "peekIssueId";
const PEEK_PROJECT_QUERY_KEY = "peekProjectId";
const PEEK_NESTING_QUERY_KEY = "peekNestingLevel";

type Props = {
  /**
   * Optional override for the issue service type. When omitted, the hook reads
   * the IssuesStoreContext to pick ISSUES vs EPICS — same convention as
   * IssuePeekOverview itself, so epic peeks sync with the EPICS store.
   */
  storeType?: EIssuesStoreType;
};

/**
 * Two-way bind between the URL query string and the peek-issue MobX store, so
 * a peeked issue is deep-linkable. Mount this once inside any layout that
 * wants the behavior — DO NOT bake the sync into the leaf IssuePeekOverview
 * (Docs, profile, home, notifications must NOT mutate the URL on peek).
 *
 * URL schema:
 *   ?peekIssueId=<id>
 *   &peekProjectId=<projectId>     (omitted when it equals the route's :projectId)
 *   &peekNestingLevel=<n>          (sub-issue peeks; preserves list/spreadsheet
 *                                   active-row highlight after URL bootstrap)
 */
export const IssuePeekUrlSync = observer(function IssuePeekUrlSync(props: Props) {
  const { storeType: storeTypeFromProps } = props;
  const routeParams = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const urlWorkspaceSlug = routeParams.workspaceSlug?.toString();
  const urlProjectId = routeParams.projectId?.toString();
  const urlPeekIssueId = searchParams.get(PEEK_ISSUE_QUERY_KEY) || undefined;
  const urlPeekProjectId = searchParams.get(PEEK_PROJECT_QUERY_KEY) || undefined;
  const urlPeekNestingRaw = searchParams.get(PEEK_NESTING_QUERY_KEY);
  const urlPeekNestingLevel = urlPeekNestingRaw ? Number(urlPeekNestingRaw) : undefined;

  const issueStoreType = useIssueStoreType();
  const storeType = storeTypeFromProps ?? issueStoreType;
  const isArchivedRoute = storeType === EIssuesStoreType.ARCHIVED;
  const serviceType = storeType === EIssuesStoreType.EPIC ? EIssueServiceType.EPICS : EIssueServiceType.ISSUES;
  const { peekIssue, setPeekIssue, isAnyModalOpen } = useIssueDetail(serviceType);

  const canSync = !!urlWorkspaceSlug;

  // URL -> store
  useEffect(() => {
    if (!canSync) return;
    if (urlPeekIssueId) {
      // Prefer URL-encoded projectId, fall back to the route param.
      const peekedProjectId = urlPeekProjectId ?? urlProjectId;
      if (!peekedProjectId) return;
      const sameIssue = peekIssue?.issueId === urlPeekIssueId && peekIssue?.projectId === peekedProjectId;
      if (sameIssue) return;
      setPeekIssue({
        workspaceSlug: urlWorkspaceSlug!,
        projectId: peekedProjectId,
        issueId: urlPeekIssueId,
        nestingLevel: Number.isFinite(urlPeekNestingLevel) ? urlPeekNestingLevel : undefined,
        isArchived: isArchivedRoute,
      });
    } else if (peekIssue?.issueId) {
      // Don't tear down an open peek out from under an active modal (delete,
      // archive, edit, relation, link, attachment confirm). Edits there
      // typically own unsaved input.
      if (isAnyModalOpen) return;
      setPeekIssue(undefined);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canSync, urlPeekIssueId, urlPeekProjectId, urlPeekNestingLevel, urlWorkspaceSlug, urlProjectId, isArchivedRoute]);

  // store -> URL
  useEffect(() => {
    if (!canSync) return;
    // Only persist peeks that "belong" to this route: same projectId on project
    // routes, or any peek on workspace-level routes (no path-level projectId
    // to compare against). Stops a stale peek carried over from another route
    // from poisoning the current URL.
    const isOurPeek = !!peekIssue && (!urlProjectId || peekIssue.projectId === urlProjectId);
    const targetPeekId = isOurPeek ? peekIssue?.issueId : undefined;
    const targetPeekProjectId = isOurPeek ? peekIssue?.projectId : undefined;
    const targetNestingLevel = isOurPeek ? peekIssue?.nestingLevel : undefined;
    const desiredProjectParam =
      targetPeekProjectId && targetPeekProjectId !== urlProjectId ? targetPeekProjectId : undefined;
    const desiredNestingParam =
      typeof targetNestingLevel === "number" && targetNestingLevel > 0 ? String(targetNestingLevel) : undefined;
    if (
      targetPeekId === urlPeekIssueId &&
      desiredProjectParam === urlPeekProjectId &&
      desiredNestingParam === urlPeekNestingRaw
    ) {
      return;
    }
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (targetPeekId) next.set(PEEK_ISSUE_QUERY_KEY, targetPeekId);
        else next.delete(PEEK_ISSUE_QUERY_KEY);
        if (desiredProjectParam) next.set(PEEK_PROJECT_QUERY_KEY, desiredProjectParam);
        else next.delete(PEEK_PROJECT_QUERY_KEY);
        if (desiredNestingParam) next.set(PEEK_NESTING_QUERY_KEY, desiredNestingParam);
        else next.delete(PEEK_NESTING_QUERY_KEY);
        return next;
      },
      { replace: true, preventScrollReset: true }
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    canSync,
    peekIssue?.issueId,
    peekIssue?.projectId,
    peekIssue?.nestingLevel,
    urlProjectId,
    urlPeekIssueId,
    urlPeekProjectId,
    urlPeekNestingRaw,
  ]);

  return null;
});
