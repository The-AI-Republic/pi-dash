// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline approval request above the composer (NEWFRONT-553, RUN-034;
// expiring requests NEWFRONT-589, DESK-008). One request at a time: the
// engine parks its turn until answered. The prompt names the narrowest
// thing being asked for and the engine's reason, and only a command offers
// the always-allow-for-session answer. A request with a deadline shows how
// long is left; the controls stay active until it runs out.

import * as React from "react";

import { Button } from "@pidash/kit";

import { APPROVAL_KIND_TEXT, approvalTarget, type PendingApproval } from "./approvalCopy.js";

/** Quiet line under the reasoning when nothing was sent along with the request. */
export const APPROVAL_NO_REASON = "No reason was given.";

/**
 * Whole seconds left before a request expires, floored at zero, or null
 * when the request does not expire. Split out so the ticking clock is easy
 * to test without rendering.
 */
export function approvalSecondsLeft(expiresAt: string | null, nowMs: number): number | null {
  if (expiresAt === null) return null;
  const deadline = Date.parse(expiresAt);
  if (!Number.isFinite(deadline)) return null;
  return Math.max(0, Math.ceil((deadline - nowMs) / 1000));
}

/** The countdown sentence; it names urgency only when the time is short. */
export function approvalCountdownText(seconds: number): string {
  if (seconds <= 0) return "This request timed out.";
  if (seconds < 60) return `Expires in ${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest === 0 ? `Expires in ${minutes}m` : `Expires in ${minutes}m ${rest}s`;
}

/** Rerender cadence for a live countdown: seconds, not every frame. */
export const APPROVAL_TICK_MS = 1_000;

/**
 * The seconds left, re-read once a second while the request is open. A
 * request that already ran out reports zero once and stops asking.
 */
export function useApprovalCountdown(expiresAt: string | null): number | null {
  const [now, setNow] = React.useState(() => Date.now());
  const left = approvalSecondsLeft(expiresAt, now);
  React.useEffect(() => {
    setNow(Date.now());
    if (left === null || left <= 0) return;
    const timer = setInterval(() => setNow(Date.now()), APPROVAL_TICK_MS);
    return () => clearInterval(timer);
  }, [expiresAt, left === null || left <= 0]);
  return left;
}

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
  const secondsLeft = useApprovalCountdown(request.expiresAt);
  const expired = secondsLeft !== null && secondsLeft <= 0;
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
      {request.reason === "" ? (
        <p data-testid="chat-approval-reason" className="mt-(--space-1) text-(--text-muted)">
          {APPROVAL_NO_REASON}
        </p>
      ) : (
        <p data-testid="chat-approval-reason" className="mt-(--space-1) text-(--text-muted)">
          {request.reason}
        </p>
      )}
      {secondsLeft === null ? null : (
        <p
          data-testid="chat-approval-countdown"
          role="timer"
          aria-live="off"
          className="text-caption mt-(--space-1) text-(--text-muted)"
        >
          {approvalCountdownText(secondsLeft)}
        </p>
      )}
      <div className="mt-(--space-2) flex flex-wrap items-center gap-(--space-2)">
        <Button variant="primary" size="small" disabled={busy || expired} onClick={() => onDecide("accept")}>
          {APPROVE_LABEL}
        </Button>
        <Button variant="secondary" size="small" disabled={busy || expired} onClick={() => onDecide("decline")}>
          {DENY_LABEL}
        </Button>
        {request.kind === "command_execution" ? (
          <Button
            variant="ghost"
            size="small"
            disabled={busy || expired}
            onClick={() => onDecide("accept_for_session")}
          >
            {ALWAYS_ALLOW_LABEL}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
