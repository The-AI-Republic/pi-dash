// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent controls on the issue surface (RUN-046, RUN-048). The runners
// area owns dispatch and the grant/refuse contract; this row only exposes
// those actions where the issue detail can use them. Re-tick follows the
// server's flag, so a refusal case never shows an affordance. Both
// controls wait for the workspace id before they can act.

import { Button } from "@pidash/kit";
import type { Issue } from "@pidash/api-client";
import { useQueryClient } from "@tanstack/react-query";
import { Play, RotateCw } from "lucide-react";
import * as React from "react";

import { useReTick, useTriggerAgentRun } from "../../runners/index.js";
import { issueKeys } from "../api/keys.js";

interface AgentActionContext {
  workspaceSlug: string;
  workspaceId: string | null;
  projectId: string;
  issue: Issue;
  /** Archived, synced and guest-visible surfaces expose no writes. */
  disabled: boolean;
}

export interface IssueRunAIActionProps extends AgentActionContext {
  /** The action-row entry keeps the issue surface's own label. */
  label?: string | undefined;
  /** Match the surrounding action strip. */
  size?: "small" | "medium" | undefined;
}

export interface IssueReTickActionProps extends AgentActionContext {}

export interface IssueAgentActionsProps extends AgentActionContext {}

function useIssueRefresh(workspaceSlug: string, issueId: string): () => Promise<unknown> {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) }),
    [issueId, queryClient, workspaceSlug]
  );
}

/** Start an agent run for this issue through the runners dispatch hook. */
export function IssueRunAIAction({
  workspaceSlug,
  workspaceId,
  projectId,
  issue,
  disabled,
  label = "Start agent",
  size = "small",
}: IssueRunAIActionProps): React.ReactElement {
  const refresh = useIssueRefresh(workspaceSlug, issue.id);
  const { triggerRun, isSubmitting } = useTriggerAgentRun({ onSettledRefresh: refresh });
  const ready = workspaceId !== null;

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

  return (
    <Button
      variant="secondary"
      size={size}
      disabled={!ready || disabled}
      loading={isSubmitting}
      data-testid="issue-run-ai"
      onClick={startRun}
    >
      <span className="flex items-center gap-(--space-1)">
        <Play size={14} strokeWidth={1.5} aria-hidden="true" />
        <span>{label}</span>
      </span>
    </Button>
  );
}

/** Ask for fresh budget when the server says a refill is owed. */
export function IssueReTickAction({
  workspaceSlug,
  workspaceId,
  issue,
  disabled,
}: IssueReTickActionProps): React.ReactElement | null {
  const refresh = useIssueRefresh(workspaceSlug, issue.id);
  const { reTick, isSubmitting } = useReTick({ onSettledRefresh: refresh });
  if (!reTickOffered(issue)) return null;
  const ready = workspaceId !== null;

  function refillBudget(): void {
    if (workspaceId === null || disabled) return;
    void reTick(issue.id);
  }

  return (
    <Button
      variant="secondary"
      size="small"
      disabled={!ready || disabled}
      loading={isSubmitting}
      data-testid="issue-re-tick"
      onClick={refillBudget}
    >
      <span className="flex items-center gap-(--space-1)">
        <RotateCw size={14} strokeWidth={1.5} aria-hidden="true" />
        <span>Refill budget</span>
      </span>
    </Button>
  );
}

/** The paragraph carrying both server actions on surfaces without a widget row. */
export function IssueAgentActions(props: IssueAgentActionsProps): React.ReactElement {
  return (
    <div className="flex flex-wrap items-center gap-(--space-2)" aria-label="Agent actions">
      <IssueRunAIAction {...props} />
      <IssueReTickAction {...props} />
    </div>
  );
}

/** The server says a refill is owed; either ticker field may carry it. */
export function reTickOffered(issue: Pick<Issue, "agent_status" | "agent_ticker">): boolean {
  const ticker = issue.agent_status?.ticker ?? issue.agent_ticker;
  return ticker?.can_re_tick === true;
}
