// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation transcript (NEWFRONT-549 RUN-026, streaming NEWFRONT-551
// RUN-029). Saved and streamed rows in sequence order, the person's turns
// on the trailing edge, assistant replies as formatted markdown that is
// never blank, and the activity strip the live feed derives beneath the
// messages. A closed conversation stays readable.

import * as React from "react";

import type { ChatMessage } from "@pidash/api-client";
import { cn, Markdown } from "@pidash/kit";

import type { ActivityRow } from "./events.js";

export const CHAT_TRANSCRIPT_EMPTY = "No messages";
export const CHAT_TRANSCRIPT_WAITING = "Waiting for the first message…";
export const CHAT_STREAMING_PLACEHOLDER = "…";

export interface ChatTranscriptProps {
  messages: readonly ChatMessage[];
  loading: boolean;
  /** Live action rows above the composer; newest last. */
  activity?: readonly ActivityRow[];
}

/** Text a plain row shows; a row with no content yet reads its state. */
export function messageText(message: Pick<ChatMessage, "content" | "status">): string {
  const content = message.content.trim();
  if (content !== "") return message.content;
  return message.status;
}

/**
 * What a bubble shows before any text exists. A streaming reply reads as
 * a placeholder rather than its bare status word, and the placeholder is
 * announced as activity, not as assistant prose.
 */
export function bubbleContent(message: Pick<ChatMessage, "content" | "status">): {
  text: string;
  streaming: boolean;
} {
  if (message.content.trim() !== "") return { text: message.content, streaming: false };
  return { text: message.status === "streaming" ? CHAT_STREAMING_PLACEHOLDER : message.status, streaming: true };
}

function MessageBubble({ message }: { message: ChatMessage }): React.ReactElement {
  if (message.role === "user") {
    return (
      <div
        data-role="user"
        className="text-body max-w-[80%] rounded-(--radius-popover) bg-(--accent) px-(--space-3) py-(--space-2) whitespace-pre-wrap text-(--on-accent)"
      >
        {message.content}
      </div>
    );
  }
  if (message.role === "tool" || message.role === "system") {
    return (
      <div
        data-role={message.role}
        className="text-meta max-w-[80%] rounded-(--radius-popover) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) whitespace-pre-wrap text-(--text-muted)"
      >
        {messageText(message)}
      </div>
    );
  }
  const content = bubbleContent(message);
  return (
    <div
      data-role="assistant"
      data-streaming={content.streaming ? "true" : undefined}
      className="text-body max-w-[80%] rounded-(--radius-popover) bg-(--subtle) px-(--space-3) py-(--space-2) text-(--text)"
    >
      {content.streaming ? (
        <span className="text-(--text-muted)">{content.text}</span>
      ) : (
        <Markdown source={content.text} />
      )}
    </div>
  );
}

export function ChatTranscript({ messages, loading, activity = [] }: ChatTranscriptProps): React.ReactElement {
  return (
    <div data-testid="runner-chat-transcript" className="flex min-h-0 flex-1 flex-col overflow-auto py-(--space-4)">
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
              <MessageBubble message={message} />
            </li>
          ))}
        </ol>
      )}
      {activity.length === 0 ? null : (
        <ul aria-label="Recent agent activity" className="mt-(--space-4) flex flex-col gap-(--space-1)">
          {activity.map((row) => (
            <li
              key={row.seq}
              data-testid="runner-chat-activity"
              className="text-meta font-mono truncate rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-1) text-(--text-muted)"
            >
              {row.label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
