// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One pending-approval card (NEWFRONT-547, RUN-023/RUN-024). The header
// names the owning run and request time, the body carries the kind
// heading with the optional reason, expiry and full payload, and the
// three decision buttons share one per-card loading flag.

import { Button } from "@pidash/kit";
import * as React from "react";

import type { ApprovalDecision, ApprovalRequest } from "@pidash/api-client";
import { approvalKindLabel } from "./approvalKind.js";

export interface ApprovalCardProps {
  approval: ApprovalRequest;
  /** True while this card's decision is in flight. */
  pending: boolean;
  onDecide: (approvalId: string, decision: ApprovalDecision) => void;
}

/** Clock time for request and expiry stamps. */
export function approvalClockTime(iso: string): string {
  return new Date(iso).toLocaleTimeString();
}

export function ApprovalCard({ approval, pending, onDecide }: ApprovalCardProps): React.ReactElement {
  return (
    <article
      data-testid="approval-card"
      data-approval-id={approval.id}
      className="rounded-(--radius-control) border border-(--border) p-(--space-4)"
    >
      <div className="flex items-start justify-between gap-(--space-3)">
        <div className="min-w-0">
          <div data-testid="approval-card-header" className="text-meta text-(--text-muted)">
            Run {approval.agent_run} · asked {approvalClockTime(approval.requested_at)}
          </div>
          <div data-testid="approval-card-kind" className="text-body font-medium text-(--text)">
            {approvalKindLabel(approval.kind)}
          </div>
          {approval.reason !== "" ? (
            <div data-testid="approval-card-reason" className="text-body mt-(--space-1) text-(--text-muted)">
              {approval.reason}
            </div>
          ) : null}
        </div>
        {approval.expires_at !== null ? (
          <div data-testid="approval-card-expiry" className="text-meta shrink-0 text-(--text-muted)">
            Ends {approvalClockTime(approval.expires_at)}
          </div>
        ) : null}
      </div>
      <pre
        data-testid="approval-card-payload"
        className="font-mono text-meta mt-(--space-3) max-h-80 overflow-auto rounded-(--radius-control) bg-(--subtle) p-(--space-3) whitespace-pre-wrap"
      >
        {JSON.stringify(approval.payload, null, 2)}
      </pre>
      <div className="mt-(--space-3) flex gap-(--space-2)">
        <Button
          size="small"
          variant="secondary"
          loading={pending}
          data-testid="approval-allow-once"
          onClick={() => onDecide(approval.id, "accept")}
        >
          Allow once
        </Button>
        <Button
          size="small"
          variant="primary"
          loading={pending}
          data-testid="approval-allow-session"
          onClick={() => onDecide(approval.id, "accept_for_session")}
        >
          Allow for session
        </Button>
        <Button
          size="small"
          variant="danger"
          loading={pending}
          data-testid="approval-deny"
          onClick={() => onDecide(approval.id, "decline")}
        >
          Deny
        </Button>
      </div>
    </article>
  );
}
