// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-transcript view (NEWFRONT-549, RUN-026). A conversation picked from
// history stays readable, including a closed one: rows render in sequence
// order with the user's turns aligned right. Streaming, markdown and the
// activity strip arrive with the messaging slice on top of this list.

import * as React from "react";

import type { ChatMessage } from "@pidash/api-client";
import { cn } from "@pidash/kit";

export const CHAT_TRANSCRIPT_EMPTY = "No messages";
export const CHAT_TRANSCRIPT_WAITING = "Waiting for the first message…";

export interface ChatTranscriptProps {
  messages: readonly ChatMessage[];
  loading: boolean;
}

/** Text shown inside a bubble; a row with no content yet reads its state. */
export function messageText(message: Pick<ChatMessage, "content" | "status">): string {
  const content = message.content.trim();
  if (content !== "") return message.content;
  return message.status;
}

export function ChatTranscript({ messages, loading }: ChatTranscriptProps): React.ReactElement {
  return (
    <div data-testid="runner-chat-transcript" className="min-h-0 flex-1 overflow-auto py-(--space-4)">
      {messages.length === 0 ? (
        <p
          data-testid="runner-chat-transcript-empty"
          className="text-body py-(--space-12) text-center text-(--text-muted)"
        >
          {loading ? CHAT_TRANSCRIPT_WAITING : CHAT_TRANSCRIPT_EMPTY}
        </p>
      ) : (
        <ol className="flex flex-col gap-(--space-3)" aria-label="Messages">
          {messages.map((message) => (
            <li key={message.id} className={cn("flex", message.role === "user" ? "justify-end" : "justify-start")}>
              <div
                data-role={message.role}
                className={cn(
                  "text-body max-w-[80%] rounded-(--radius-popover) px-(--space-3) py-(--space-2) whitespace-pre-wrap",
                  message.role === "user" ? "bg-(--accent) text-(--on-accent)" : "bg-(--subtle) text-(--text)"
                )}
              >
                {messageText(message)}
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
