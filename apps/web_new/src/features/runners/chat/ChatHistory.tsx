// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation history column (NEWFRONT-549, RUN-026). Newest activity
// first, each entry labelled by when it last moved with relative age and
// any non-open status underneath, and one control to start a fresh
// conversation. Purely presentational: the page owns the data and the
// selection.

import { MessageSquare, SquarePen } from "lucide-react";
import * as React from "react";

import { Button, cn } from "@pidash/kit";

import type { ChatHistoryEntry } from "./chatState.js";

export const CHAT_HISTORY_HEADING = "Chats";
export const CHAT_NEW_LABEL = "New chat";
export const CHAT_HISTORY_EMPTY = "No chats yet.";

export interface ChatHistoryProps {
  heading?: string;
  entries: readonly ChatHistoryEntry[];
  onSelect: (sessionId: string) => void;
  onNewChat: () => void;
  /** Disables the New chat control while a create is outstanding. */
  busy?: boolean;
}

export function ChatHistory({
  heading = CHAT_HISTORY_HEADING,
  entries,
  onSelect,
  onNewChat,
  busy = false,
}: ChatHistoryProps): React.ReactElement {
  return (
    <aside
      aria-label="Conversation history"
      data-testid="runner-chat-history"
      className="flex w-64 shrink-0 flex-col border-r border-(--border) bg-(--surface)"
    >
      <div className="shrink-0 p-(--space-2)">
        <Button
          variant="ghost"
          size="small"
          className="w-full justify-start"
          disabled={busy}
          loading={busy}
          onClick={onNewChat}
        >
          {busy ? null : <SquarePen size={16} strokeWidth={1.5} aria-hidden="true" />}
          <span>{CHAT_NEW_LABEL}</span>
        </Button>
      </div>
      <div className="text-meta px-(--space-4) pb-(--space-1) font-medium tracking-wide text-(--text-muted) uppercase">
        {heading}
      </div>
      <nav
        aria-label="Previous conversations"
        className="flex min-h-0 flex-1 flex-col gap-(--space-1) overflow-y-auto p-(--space-2)"
      >
        {entries.length === 0 ? (
          <p
            data-testid="runner-chat-history-empty"
            className="text-meta px-(--space-2) py-(--space-4) text-(--text-muted)"
          >
            {CHAT_HISTORY_EMPTY}
          </p>
        ) : (
          entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              aria-current={entry.active ? "true" : undefined}
              onClick={() => onSelect(entry.id)}
              className={cn(
                "flex min-h-11 w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left",
                "hover:bg-(--subtle) focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid",
                entry.active ? "bg-(--subtle) font-medium text-(--text)" : "text-(--text-muted)"
              )}
            >
              <MessageSquare size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0" />
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate">{entry.title}</span>
                <span className="text-meta truncate text-(--text-muted)">{entry.subtitle}</span>
              </span>
            </button>
          ))
        )}
      </nav>
    </aside>
  );
}
