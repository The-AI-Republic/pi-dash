// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat header (NEWFRONT-549, RUN-032). Names the runner, shows one
// secondary line (the session's working directory, else its pod, else the
// runner's state) and the status badge. The approval-mode control belongs
// to the desktop chat slice; the cloud transport has no approvals.

import * as React from "react";

import { RunnerStatusBadge } from "../components/RunnerStatusBadge.js";
import type { ChatHeaderSummary } from "./chatState.js";

export interface ChatHeaderProps {
  summary: ChatHeaderSummary;
  /** Extra controls at the trailing edge (stop/close arrive with messaging). */
  actions?: React.ReactNode;
}

export function ChatHeader({ summary, actions }: ChatHeaderProps): React.ReactElement {
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
      </div>
    </header>
  );
}
