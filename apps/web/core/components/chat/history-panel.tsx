/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import { MessageSquare, SquarePen } from "lucide-react";
import { cn } from "@pi-dash/utils";

export interface ChatHistoryItem {
  id: string;
  /** Primary label for the entry (e.g. chat title, first message, or a date). */
  title: string;
  /** Secondary line — typically a timestamp or status. */
  subtitle?: string;
  /** Whether this entry is the currently loaded conversation. */
  active?: boolean;
}

interface ChatHistoryPanelProps {
  /** Section heading shown above the list (e.g. "Chats"). */
  heading: string;
  items: ChatHistoryItem[];
  onSelect: (id: string) => void;
  onNewChat: () => void;
  newChatLabel?: string;
  /** Rendered in place of the list when there are no items. */
  emptyState?: ReactNode;
  /** Disables the New Chat button while a session is being created. */
  busy?: boolean;
  className?: string;
}

/**
 * Left-column chat history panel shared by the Pi Dash AI and AI runner chat
 * pages. Lists prior conversations (clickable to load) and exposes a New Chat
 * action. Purely presentational — the host owns session state and data fetching.
 */
export function ChatHistoryPanel({
  heading,
  items,
  onSelect,
  onNewChat,
  newChatLabel = "New chat",
  emptyState,
  busy = false,
  className,
}: ChatHistoryPanelProps) {
  return (
    <aside className={cn("flex w-64 shrink-0 flex-col border-r border-subtle bg-surface-1", className)}>
      <div className="shrink-0 p-2">
        <button
          type="button"
          onClick={onNewChat}
          disabled={busy}
          className="flex h-9 w-full items-center gap-2 rounded px-2 text-13 text-secondary hover:bg-layer-1 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <SquarePen className="size-4 shrink-0" />
          <span>{newChatLabel}</span>
        </button>
      </div>
      <nav className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto p-2 pt-0">
        <div className="px-2 py-1 text-11 font-medium text-tertiary uppercase">{heading}</div>
        {items.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => onSelect(item.id)}
            className={cn(
              "flex min-h-9 items-center gap-2 rounded px-2 py-2 text-left text-13",
              item.active ? "bg-layer-1 font-medium text-primary" : "text-secondary hover:bg-layer-1"
            )}
          >
            <MessageSquare className="size-4 shrink-0" />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate">{item.title}</span>
              {item.subtitle && <span className="truncate text-11 text-tertiary">{item.subtitle}</span>}
            </span>
          </button>
        ))}
        {items.length === 0 &&
          (emptyState ?? <div className="px-2 py-4 text-12 text-tertiary">No conversations yet.</div>)}
      </nav>
    </aside>
  );
}
