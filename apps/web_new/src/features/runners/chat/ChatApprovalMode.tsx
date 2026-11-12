// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Approval-mode control in the chat header (NEWFRONT-553, RUN-035). A
// three-way segmented group, most cautious first; the active mode reads as
// pressed. Changing it only affects the next thread the engine spawns.

import * as React from "react";

import { cn } from "@pidash/kit";

import { APPROVAL_MODE_COPY, APPROVAL_MODES, type ApprovalMode } from "./approvalCopy.js";

export const APPROVAL_MODE_GROUP_LABEL = "Approval mode";

export interface ChatApprovalModeProps {
  value: ApprovalMode;
  onChange: (mode: ApprovalMode) => void;
  /** Disabled while the runtime is being prepared. */
  disabled?: boolean;
}

export function ChatApprovalMode({ value, onChange, disabled = false }: ChatApprovalModeProps): React.ReactElement {
  return (
    <div
      role="group"
      aria-label={APPROVAL_MODE_GROUP_LABEL}
      data-testid="chat-approval-mode"
      className="flex items-center gap-(--space-1)"
    >
      <span className="text-caption text-(--text-muted)">Approvals</span>
      {APPROVAL_MODES.map((mode) => {
        const copy = APPROVAL_MODE_COPY[mode];
        const active = mode === value;
        return (
          <button
            key={mode}
            type="button"
            title={copy.help}
            aria-pressed={active}
            disabled={disabled}
            onClick={() => onChange(mode)}
            className={cn(
              "text-caption cursor-pointer rounded-(--radius-control) px-(--space-2) py-(--space-1) transition-colors disabled:cursor-not-allowed disabled:opacity-50",
              active
                ? "bg-(--subtle) font-medium text-(--text)"
                : "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
            )}
          >
            {copy.label}
          </button>
        );
      })}
    </div>
  );
}
