// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat header (NEWFRONT-549 RUN-032, close NEWFRONT-551 RUN-031). Names
// the runner, shows one secondary line (the session's working directory,
// else its pod, else the runner's state) and the status badge, plus the
// close control whenever a conversation is on screen. The approval-mode
// control belongs to the desktop chat slice.

import { X } from "lucide-react";
import * as React from "react";

import { IconButton } from "@pidash/kit";

import { RunnerStatusBadge } from "../components/RunnerStatusBadge.js";
import type { ChatHeaderSummary } from "./chatState.js";

export const CHAT_CLOSE_LABEL = "Close chat";

export interface ChatHeaderProps {
  summary: ChatHeaderSummary;
  /** Extra controls at the trailing edge. */
  actions?: React.ReactNode;
  /** Closes the conversation; absent while none is resolved. */
  onClose?: (() => void) | undefined;
}

export function ChatHeader({ summary, actions, onClose }: ChatHeaderProps): React.ReactElement {
  return (
    <header
      data-testid="runner-chat-header"
      className="flex h-14 shrink-0 items-center justify-between gap-(--space-4) border-b border-(--border)"
    >
      <div className="min-w-0">
        <h1 data-testid="runner-chat-name" className="text-emphasis truncate font-semibold text-(--text)">
          {summary.name}
        </h1>
        {summary.secondary === "" ? null : (
          <div
            data-testid="runner-chat-secondary"
            title={summary.secondary}
            className="text-meta truncate text-(--text-muted)"
          >
            {summary.secondary}
          </div>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-(--space-2)">
        {summary.status === null ? null : <RunnerStatusBadge status={summary.status} />}
        {actions}
        {onClose === undefined ? null : (
          <IconButton label={CHAT_CLOSE_LABEL} onClick={onClose}>
            <X size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        )}
      </div>
    </header>
  );
}
