// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent controls on the issue surface (RUN-046, RUN-048). The runners
// area owns dispatch and the grant/refuse contract; this row only exposes
// those actions where the issue detail can use them. Re-tick follows the
// server's flag, so a refusal case never shows an affordance.

import { Button } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { Play, RotateCw } from "lucide-react";
import * as React from "react";

import { useReTick, useTriggerAgentRun } from "../../runners/index.js";
import { issueKeys } from "../api/keys.js";

export interface IssueAgentActionsProps {
  workspaceSlug: string;
  /** Resolved by the issue route; absent on surfaces without a session. */
  workspaceId: string | null;
  projectId: string;
  issue: Issue;
  /** Archived, synced and guest-visible surfaces expose no writes. */
  disabled: boolean;
}

/** The paragraph carrying the two server actions. */
export function IssueAgentActions({
  workspaceSlug,
  workspaceId,
  projectId,
  issue,
  disabled,
}: IssueAgentActionsProps): React.ReactElement {
  const queryClient = useQueryClient();
  const refresh = React.useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issue.id) });
  }, [issue.id, queryClient, workspaceSlug]);
  const { triggerRun, isSubmitting: isStarting } = useTriggerAgentRun({ onSettledRefresh: refresh });
  const { reTick, isSubmitting: isReTicking } = useReTick({ onSettledRefresh: refresh });
  const canStart = workspaceId !== null && !disabled;
  const canReTick = reTickOffered(issue);

  function startRun(): void {
    if (workspaceId === null || disabled) return;
    void triggerRun({
      workspaceId,
      workspaceSlug,
      projectId,
      issueId: issue.id,
      mode: "run_ai",
    });
  }

  function refillBudget(): void {
    if (!canReTick || disabled) return;
    void reTick(issue.id);
  }

  return (
    <div className="flex flex-wrap items-center gap-(--space-2)" aria-label="Agent actions">
      <Button
        variant="secondary"
        size="small"
        disabled={!canStart}
        loading={isStarting}
        data-testid="issue-run-ai"
        onClick={startRun}
      >
        <span className="flex items-center gap-(--space-1)">
          <Play size={14} strokeWidth={1.5} aria-hidden="true" />
          <span>Start agent</span>
        </span>
      </Button>
      {canReTick ? (
        <Button
          variant="secondary"
          size="small"
          disabled={disabled}
          loading={isReTicking}
          data-testid="issue-re-tick"
          onClick={refillBudget}
        >
          <span className="flex items-center gap-(--space-1)">
            <RotateCw size={14} strokeWidth={1.5} aria-hidden="true" />
            <span>Refill budget</span>
          </span>
        </Button>
      ) : null}
    </div>
  );
}

/** The server says a refill is owed; either ticker field may carry it. */
export function reTickOffered(issue: Pick<Issue, "agent_status" | "agent_ticker">): boolean {
  const ticker = issue.agent_status?.ticker ?? issue.agent_ticker;
  return ticker?.can_re_tick === true;
}
