// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation history column (NEWFRONT-549, RUN-026; on-machine history
// NEWFRONT-589, DESK-009). Newest activity first, each entry labelled by
// its own name or by when it last moved, with relative age and any
// non-open status underneath, and one control to start a fresh
// conversation. When the surface keeps history on the machine, each row
// offers rename and delete and the column offers a clear-all control.
// Purely presentational: the page owns the data, the selection and the
// writes.

import { Eraser, MessageSquare, MoreHorizontal, SquarePen } from "lucide-react";
import * as React from "react";

import { Button, IconButton, Menu, cn } from "@pidash/kit";

import type { ChatHistoryEntry } from "./chatState.js";
import { CLEAR_ACTION, DELETE_ACTION, RENAME_ACTION } from "./historyActions.js";

export const CHAT_HISTORY_HEADING = "Chats";
export const CHAT_NEW_LABEL = "New chat";
export const CHAT_HISTORY_EMPTY = "No chats yet.";
export const CHAT_ROW_MENU_LABEL = "Chat actions";

export interface ChatHistoryProps {
  heading?: string;
  entries: readonly ChatHistoryEntry[];
  onSelect: (sessionId: string) => void;
  onNewChat: () => void;
  /** Disables the New chat control while a create is outstanding. */
  busy?: boolean;
  /**
   * Present when the surface keeps history on this machine: rows gain
   * rename and delete, and the column a clear-all control.
   */
  onRename?: ((entry: ChatHistoryEntry) => void) | undefined;
  onDelete?: ((entry: ChatHistoryEntry) => void) | undefined;
  onClear?: (() => void) | undefined;
}

export function ChatHistory({
  heading = CHAT_HISTORY_HEADING,
  entries,
  onSelect,
  onNewChat,
  busy = false,
  onRename,
  onDelete,
  onClear,
}: ChatHistoryProps): React.ReactElement {
  const manageable = onRename !== undefined || onDelete !== undefined;
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
      <div className="flex items-center justify-between gap-(--space-2) px-(--space-4) pb-(--space-1)">
        <span className="text-meta font-medium tracking-wide text-(--text-muted) uppercase">{heading}</span>
        {onClear === undefined ? null : (
          <IconButton
            label={CLEAR_ACTION}
            size="small"
            onClick={onClear}
            disabled={entries.length === 0}
            data-testid="runner-chat-history-clear"
          >
            <Eraser size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        )}
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
            <div key={entry.id} className="group/history relative">
              <button
                type="button"
                aria-current={entry.active ? "true" : undefined}
                onClick={() => onSelect(entry.id)}
                className={cn(
                  "flex min-h-11 w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left",
                  "hover:bg-(--subtle) focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid",
                  entry.active ? "bg-(--subtle) font-medium text-(--text)" : "text-(--text-muted)",
                  manageable ? "pr-(--space-8)" : null
                )}
              >
                <MessageSquare size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate">{entry.title}</span>
                  <span className="text-meta truncate text-(--text-muted)">{entry.subtitle}</span>
                </span>
              </button>
              {manageable ? <ChatRowMenu entry={entry} onRename={onRename} onDelete={onDelete} /> : null}
            </div>
          ))
        )}
      </nav>
    </aside>
  );
}

interface ChatRowMenuProps {
  entry: ChatHistoryEntry;
  onRename: ((entry: ChatHistoryEntry) => void) | undefined;
  onDelete: ((entry: ChatHistoryEntry) => void) | undefined;
}

/**
 * The row's actions. A conversation with a reply streaming is still
 * selectable, but acting on it waits: the row says why instead of letting
 * a rename land under a turn that is writing to it.
 */
function ChatRowMenu({ entry, onRename, onDelete }: ChatRowMenuProps): React.ReactElement {
  const disabled = entry.busyReason !== null;
  const items = [
    ...(onRename === undefined ? [] : [{ label: RENAME_ACTION, onSelect: () => onRename(entry), disabled }]),
    ...(onDelete === undefined ? [] : [{ label: DELETE_ACTION, onSelect: () => onDelete(entry), disabled }]),
  ];
  return (
    <span
      className="absolute top-1/2 right-(--space-1) -translate-y-1/2"
      {...(entry.busyReason === null ? {} : { title: entry.busyReason })}
    >
      <Menu
        trigger={
          <IconButton label={`${CHAT_ROW_MENU_LABEL}: ${entry.title}`} size="small" data-testid="runner-chat-row-menu">
            <MoreHorizontal size={14} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={items}
      />
    </span>
  );
}
