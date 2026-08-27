/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import { cn } from "@pi-dash/utils";
import { type ChatListItem, ChatMessageList } from "@/components/chat/message-list";

interface ChatContainerProps<M extends ChatListItem> {
  messages: M[];
  renderMessage?: (message: M) => ReactNode;
  emptyState?: ReactNode;
  /** Extra content rendered after the messages (e.g. a runner debug event strip). */
  listFooter?: ReactNode;
  /** Optional header row above the message list (e.g. the runner's title bar). */
  header?: ReactNode;
  /** Optional inline notice rendered between the list and the composer (e.g. an error line). */
  error?: ReactNode;
  /** The composer element for this surface (each view owns its own props/handlers). */
  composer: ReactNode;
  /** Override the outer height/layout when a surface needs it (e.g. a min-height). */
  className?: string;
}

/**
 * Shared chat shell that assembles the message list, an optional header/footer,
 * an optional inline error, and the composer into the standard full-height
 * chat layout. Both the Pi Dash AI assistant chat and the runner agent chat
 * render through this so layout and streaming/scroll behavior improve in one
 * place; surface-specific differences are supplied via props/slots.
 */
export function ChatContainer<M extends ChatListItem>({
  messages,
  renderMessage,
  emptyState,
  listFooter,
  header,
  error,
  composer,
  className,
}: ChatContainerProps<M>) {
  return (
    <div className={cn("flex h-full min-h-0 flex-col overflow-hidden", className)}>
      {header}
      <ChatMessageList messages={messages} renderMessage={renderMessage} emptyState={emptyState} footer={listFooter} />
      {error}
      {composer}
    </div>
  );
}
