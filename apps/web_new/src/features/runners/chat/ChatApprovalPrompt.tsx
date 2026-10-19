// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline approval request above the composer (NEWFRONT-553, RUN-034). One
// request at a time: the engine parks its turn until answered. The
// prompt names the narrowest thing being asked for and the engine's
// reason, and only a command offers the always-allow-for-session answer.

import * as React from "react";

import { Button } from "@pidash/kit";

import { APPROVAL_KIND_TEXT, approvalTarget, type PendingApproval } from "./approvalCopy.js";

export const APPROVAL_PROMPT_LABEL = "Approval requested";
export const APPROVE_LABEL = "Approve";
export const DENY_LABEL = "Deny";
export const ALWAYS_ALLOW_LABEL = "Always allow this session";

export interface ChatApprovalPromptProps {
  request: PendingApproval;
  onDecide: (decision: "accept" | "decline" | "accept_for_session") => void;
  /** A decision is on its way; the buttons stay inert until it lands. */
  busy?: boolean;
}

export function ChatApprovalPrompt({ request, onDecide, busy = false }: ChatApprovalPromptProps): React.ReactElement {
  const target = approvalTarget(request);
  return (
    <div
      role="alertdialog"
      aria-label={APPROVAL_PROMPT_LABEL}
      data-testid="chat-approval-prompt"
      className="text-meta mb-(--space-2) shrink-0 rounded-(--radius-control) border border-(--warning) bg-(--subtle) px-(--space-3) py-(--space-2)"
    >
      <p className="text-(--text)">The agent {APPROVAL_KIND_TEXT[request.kind]}.</p>
      {target === null ? null : (
        <code
          data-testid="chat-approval-target"
          className="font-mono text-caption mt-(--space-1) block overflow-x-auto rounded-(--radius-control) bg-(--bg) px-(--space-2) py-(--space-1)"
        >
          {target}
        </code>
      )}
      {request.reason === "" ? null : (
        <p data-testid="chat-approval-reason" className="mt-(--space-1) text-(--text-muted)">
          {request.reason}
        </p>
      )}
      <div className="mt-(--space-2) flex flex-wrap items-center gap-(--space-2)">
        <Button variant="primary" size="small" disabled={busy} onClick={() => onDecide("accept")}>
          {APPROVE_LABEL}
        </Button>
        <Button variant="secondary" size="small" disabled={busy} onClick={() => onDecide("decline")}>
          {DENY_LABEL}
        </Button>
        {request.kind === "command_execution" ? (
          <Button variant="ghost" size="small" disabled={busy} onClick={() => onDecide("accept_for_session")}>
            {ALWAYS_ALLOW_LABEL}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
