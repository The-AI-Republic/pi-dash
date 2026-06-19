/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { type ReactNode, useEffect, useRef } from "react";

export interface ChatListItem {
  id: string;
  role: string;
  content: string;
  status: string;
  seq: number;
}

interface ChatMessageListProps<M extends ChatListItem> {
  messages: M[];
  renderMessage?: (message: M) => ReactNode;
  emptyState?: ReactNode;
  /** Extra content rendered after the messages (e.g. a debug event strip). */
  footer?: ReactNode;
}

function DefaultBubble({ message }: { message: ChatListItem }) {
  const isUser = message.role === "user";
  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
      <div
        className={`max-w-[72%] rounded-md px-3 py-2 text-13 whitespace-pre-wrap ${
          isUser ? "bg-accent-primary text-on-color" : "bg-layer-1 text-primary"
        }`}
      >
        {message.content || message.status}
      </div>
    </div>
  );
}

export function ChatMessageList<M extends ChatListItem>({
  messages,
  renderMessage,
  emptyState,
  footer,
}: ChatMessageListProps<M>) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages]);

  return (
    <div className="min-h-0 flex-1 overflow-auto py-4">
      {messages.length === 0 ? (
        (emptyState ?? <div className="py-16 text-center text-13 text-secondary">No messages yet</div>)
      ) : (
        <div className="flex flex-col gap-3">
          {messages.map((message) => (
            <div key={message.id}>{renderMessage ? renderMessage(message) : <DefaultBubble message={message} />}</div>
          ))}
          {footer}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  );
}
