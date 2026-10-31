// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approvals screen container (NEWFRONT-547, RUN-023/RUN-024). One
// component serves the workspace and project scopes through the area
// context. Deciding marks one card pending; success re-reads the area
// so the card drops off, failure toasts and the card stays.

import * as React from "react";

import type { ApprovalDecision } from "@pidash/api-client";
import { notifyError } from "../../../shared/shell/index.js";
import { useDecideApproval, useApprovals } from "../api/approvals.js";
import { useRunnersArea } from "../area.js";
import { ApprovalsQueue } from "./ApprovalsQueue.js";

/** Failure title when a decision does not reach the server. */
export const APPROVAL_DECIDE_FAILED_TITLE = "Couldn't save the decision";
const APPROVAL_DECIDE_FAILED_BODY = "The approval is still waiting — try again.";

export function ApprovalsScreen(): React.ReactElement {
  const area = useRunnersArea();
  const queue = useApprovals(area.scope.workspaceSlug, area.scope.projectId ?? null);
  const decide = useDecideApproval(area.scope.workspaceSlug);
  const [pendingId, setPendingId] = React.useState<string | null>(null);

  async function decideApproval(approvalId: string, decision: ApprovalDecision): Promise<void> {
    setPendingId(approvalId);
    try {
      await decide.mutateAsync({ approvalId, decision });
    } catch {
      notifyError(APPROVAL_DECIDE_FAILED_TITLE, APPROVAL_DECIDE_FAILED_BODY);
    } finally {
      setPendingId(null);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <ApprovalsQueue
        approvals={queue.data}
        loading={queue.isPending}
        pendingId={pendingId}
        onDecide={(approvalId, decision) => void decideApproval(approvalId, decision)}
      />
    </div>
  );
}
