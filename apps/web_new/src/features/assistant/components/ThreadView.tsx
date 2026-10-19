// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation transcript (parity/specs/agents.md §10, AGT-042/AGT-043/
// AGT-047/AGT-049/AGT-053). Rows render by role: the user's turns sit on one
// side as plain text, the assistant's on the other as markdown (an ellipsis
// while a reply has no text yet), and error turns read as a tinted box. The
// list follows the tail as rows arrive, an empty thread shows the hint, and
// the docked composer owns send, stop and the keyboard contract. The
// landing's first message arrives as one-shot navigation state and is
// delivered here, then the state is cleared so a refresh never resends it.

import { Spinner } from "@pidash/kit";
import type { AssistantMessage } from "@pidash/api-client";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { markdownHtml } from "../../../shared/markdown/index.js";
import { useAssistantChat } from "../hooks/useAssistantChat.js";
import { EMPTY_CONVERSATION_HINT, HANDOFF_FAILED_TITLE } from "../lib/copy.js";
import { startFailureDetail } from "../api/mutations.js";
import { notifyError } from "../../../shared/shell/index.js";
import { readPendingMessage } from "../lib/navigation.js";
import { streamingPlaceholder } from "../lib/turn.js";
import { Composer } from "./Composer.js";
import { SetupCard } from "./SetupCard.js";

export interface ThreadViewProps {
  workspaceSlug: string;
  threadId: string;
  /** Router state carried by the navigation that opened this conversation. */
  state: unknown;
  /** False only when no provider key is stored; then the setup card fills the thread. */
  configured: boolean;
}

/** Why a bubble carries `text-danger` (the parity error-tint probe). */
const ERROR_BUBBLE_CLASS = "text-danger";

export function ThreadView({ workspaceSlug, threadId, state, configured }: ThreadViewProps): React.ReactElement {
  const chat = useAssistantChat(workspaceSlug, threadId);
  const navigate = useNavigate();
  const [draft, setDraft] = React.useState("");
  const handoff = readPendingMessage(state);
  const deliveredFor = React.useRef<string | null>(null);
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const tailRef = React.useRef<HTMLDivElement>(null);

  const sendHandoff = chat.sendHandoff;
  React.useEffect(() => {
    if (handoff === null || deliveredFor.current === threadId) return;
    deliveredFor.current = threadId;
    // Replace the history entry without the handoff before sending, so a
    // refresh or a back navigation can never deliver it twice (AGT-053).
    void navigate({ href: "", replace: true, state: {} });
    sendHandoff(handoff, (error) => notifyError(HANDOFF_FAILED_TITLE, startFailureDetail(error)));
  }, [handoff, navigate, sendHandoff, threadId]);

  // Follow the tail as rows land. The body is the scrollport; scrolling it
  // directly keeps the newest row visible without moving the page itself.
  React.useEffect(() => {
    followTail(bodyRef.current, tailRef.current);
  }, [chat.messages]);

  const submit = (content: string): void => {
    setDraft("");
    chat.send(content);
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div
        data-testid="assistant-thread-body"
        ref={bodyRef}
        className="min-h-0 flex-1 overflow-y-auto px-(--space-6) py-(--space-6)"
      >
        {chat.loaded ? (
          chat.messages.length === 0 ? (
            configured ? (
              <p
                data-testid="assistant-empty-hint"
                className="text-body mx-auto max-w-xl py-(--space-16) text-center text-(--text-muted)"
              >
                {EMPTY_CONVERSATION_HINT}
              </p>
            ) : (
              <SetupCard />
            )
          ) : (
            <ol
              data-testid="assistant-transcript"
              aria-label="Messages"
              className="mx-auto flex max-w-3xl flex-col gap-(--space-3)"
            >
              {chat.messages.map((message) => (
                <li
                  key={message.id}
                  data-seq={message.seq}
                  className={message.role === "user" ? "flex justify-end" : "flex justify-start"}
                >
                  <MessageBubble message={message} />
                </li>
              ))}
            </ol>
          )
        ) : (
          <Spinner label="Loading conversation" />
        )}
        <div ref={tailRef} aria-hidden="true" />
      </div>
      <div className="mx-auto w-full max-w-3xl shrink-0 px-(--space-6) pb-(--space-4)">
        {chat.error === null ? null : (
          <p
            role="alert"
            data-testid="assistant-error-line"
            className={`text-meta mb-(--space-2) ${ERROR_BUBBLE_CLASS}`}
          >
            {chat.error}
          </p>
        )}
        <Composer
          draft={draft}
          onDraftChange={setDraft}
          onSubmit={submit}
          posting={chat.sending}
          running={chat.running}
          onStop={chat.stop}
          configured={configured}
        />
      </div>
    </div>
  );
}

/**
 * Keep the newest row in view. The body is the scrollport, so it is scrolled
 * directly; the tail marker is scrolled into view when the host implements
 * it (jsdom does not), and either element may be absent on an early render.
 */
export function followTail(body: HTMLElement | null, tail: HTMLElement | null): void {
  if (body === null || tail === null) return;
  if (typeof tail.scrollIntoView === "function") tail.scrollIntoView({ block: "end" });
  body.scrollTop = body.scrollHeight;
}

function MessageBubble({ message }: { message: AssistantMessage }): React.ReactElement {
  const html = React.useMemo(
    () => (message.role === "assistant" && message.content.trim() !== "" ? markdownHtml(message.content) : null),
    [message.content, message.role]
  );

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
  if (message.role === "error") {
    return (
      <div
        data-role="error"
        className={`text-meta max-w-[80%] rounded-(--radius-popover) border border-(--danger) bg-(--surface) px-(--space-3) py-(--space-2) ${ERROR_BUBBLE_CLASS}`}
      >
        {message.content.trim() === "" ? "The assistant hit an error." : message.content}
      </div>
    );
  }
  if (message.role === "tool_call" || message.role === "tool_result") {
    return (
      <div data-role={message.role} className="text-meta max-w-[80%] text-(--text-muted)">
        {message.content}
      </div>
    );
  }
  return (
    <div
      data-role="assistant"
      className="text-body max-w-[80%] rounded-(--radius-popover) bg-(--subtle) px-(--space-3) py-(--space-2) text-(--text)"
    >
      {html === null ? (
        <span className="text-(--text-muted)">{streamingPlaceholder(message.status)}</span>
      ) : (
        <div className="rich-text-content" dangerouslySetInnerHTML={{ __html: html }} />
      )}
    </div>
  );
}
