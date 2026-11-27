// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval-mode control in the chat composer's toolbar (NEWFRONT-553,
// RUN-035). A compact dropdown listing the three modes most cautious
// first; the chosen mode's sentence rides the tooltip. Changing it only
// affects the next thread the engine spawns, which the tooltip says too.
// A native <select> never submits a surrounding form.

import * as React from "react";

import { cn } from "@pidash/kit";

import { APPROVAL_MODE_COPY, APPROVAL_MODES, isApprovalMode, type ApprovalMode } from "./approvalCopy.js";

export const APPROVAL_MODE_LABEL = "Approval mode";

/** Tooltip suffix: a running reply keeps the mode it started with. */
export const APPROVAL_MODE_NEXT_THREAD_NOTE = "Applies from the next conversation the agent starts.";

export interface ChatApprovalModeProps {
  value: ApprovalMode;
  onChange: (mode: ApprovalMode) => void;
  /** Disabled while the runtime is being prepared. */
  disabled?: boolean;
  className?: string;
}

export function ChatApprovalMode({
  value,
  onChange,
  disabled = false,
  className,
}: ChatApprovalModeProps): React.ReactElement {
  return (
    <select
      data-testid="chat-approval-mode"
      aria-label={APPROVAL_MODE_LABEL}
      title={`${APPROVAL_MODE_COPY[value].help} ${APPROVAL_MODE_NEXT_THREAD_NOTE}`}
      value={value}
      disabled={disabled}
      onChange={(event) => {
        const next = event.target.value;
        if (isApprovalMode(next)) onChange(next);
      }}
      className={cn(
        "text-meta max-w-48 cursor-pointer truncate rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) text-(--text) disabled:cursor-not-allowed disabled:opacity-50",
        className
      )}
    >
      {APPROVAL_MODES.map((mode) => (
        <option key={mode} value={mode} title={APPROVAL_MODE_COPY[mode].help}>
          {APPROVAL_MODE_COPY[mode].label}
        </option>
      ))}
    </select>
  );
}
