// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-approval queue list (NEWFRONT-547, RUN-023). Cards stack
// newest first as the server sends them; the empty marker exists only
// once loading has finished so waiters never settle on a spinner.

import { Spinner } from "@pidash/kit";
import * as React from "react";

import type { ApprovalDecision, ApprovalRequest } from "@pidash/api-client";
import { ApprovalCard } from "./ApprovalCard.js";

export interface ApprovalsQueueProps {
  approvals: ApprovalRequest[] | undefined;
  loading: boolean;
  /** The acted card's id, or null while nothing is deciding. */
  pendingId: string | null;
  onDecide: (approvalId: string, decision: ApprovalDecision) => void;
}

export const APPROVALS_EMPTY_TEXT = "Nothing waiting for review.";

export function ApprovalsQueue({ approvals, loading, pendingId, onDecide }: ApprovalsQueueProps): React.ReactElement {
  const rows = approvals ?? [];
  return (
    <section aria-label="Approvals" data-testid="approvals-list" className="flex min-h-0 flex-1 flex-col">
      <h1 className="text-h2 px-(--space-6) pt-(--space-6) text-(--text)">Approvals</h1>
      <div className="mt-(--space-4) min-h-0 flex-1 overflow-auto px-(--space-6) pb-(--space-6)">
        {rows.length === 0 ? (
          <div
            data-testid={loading ? undefined : "approvals-empty"}
            className="rounded-(--radius-control) border border-(--border) p-(--space-8) text-center text-(--text-muted)"
          >
            {loading ? <Spinner size="small" /> : APPROVALS_EMPTY_TEXT}
          </div>
        ) : (
          <div className="flex flex-col gap-(--space-4)">
            {rows.map((approval) => (
              <ApprovalCard
                key={approval.id}
                approval={approval}
                pending={pendingId === approval.id}
                onDecide={onDecide}
              />
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
