// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation transcript (parity/specs/agents.md §10/§11, AGT-042/AGT-043/
// AGT-046/AGT-047/AGT-049/AGT-053). Entries render by kind: the user's turns
// sit on one side as plain text, the assistant's on the other as markdown
// (an ellipsis while a reply has no text yet), error turns read as a tinted
// box, tool calls and results read as activity rows with their deep links,
// and dropped tool servers read as one inline notice line per server. The
// list follows the tail as entries arrive, an empty thread shows the hint,
// and the docked composer owns send, stop and the keyboard contract. The
// landing's first message arrives as one-shot navigation state and is
// delivered here, then the state is cleared so a refresh never resends it.

import { Spinner } from "@pidash/kit";
import type { AssistantMessage } from "@pidash/api-client";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { markdownHtml } from "../../../shared/markdown/index.js";
import { appendDictation } from "../../../shared/dictation/index.js";
import { useComposerDictation } from "../hooks/useComposerDictation.js";
import { useAssistantChat } from "../hooks/useAssistantChat.js";
import { EMPTY_CONVERSATION_HINT, HANDOFF_FAILED_TITLE, TOOL_LINK_FALLBACK } from "../lib/copy.js";
import { startFailureDetail } from "../api/mutations.js";
import { notifyError } from "../../../shared/shell/index.js";
import { readPendingMessage } from "../lib/navigation.js";
import { skippedServerLine, type ThreadNotice } from "../lib/notices.js";
import { EMPTY_REPLY_PLACEHOLDER } from "../lib/turn.js";
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

/**
 * Error surfaces carry the app's danger token, the same tint the rest of
 * web_new uses for inline failures; the parity probe reads the computed
 * colour, not a class name (AGT-047).
 */
const ERROR_TEXT_CLASS = "text-(--danger)";

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

  // Follow the tail as entries land. The body is the scrollport; scrolling
  // it directly keeps the newest row visible without moving the page
  // itself. Entries, not messages: a notice-only arrival still extends the
  // list.
  React.useEffect(() => {
    followTail(bodyRef.current, tailRef.current);
  }, [chat.entries]);

  // A reply's markdown renders after its row lands, so the list grows again
  // a moment after the scroll above. Keep the tail in view while the
  // scrollport keeps growing, but stop once the reader scrolls away: their
  // position wins from then on (AGT-047).
  React.useEffect(() => followTailWhileGrowing(bodyRef.current, tailRef.current), []);

  const submit = (content: string): void => {
    setDraft("");
    chat.send(content);
  };

  const appendTranscript = React.useCallback((text: string) => {
    setDraft((current) => appendDictation(current, text));
  }, []);
  const dictation = useComposerDictation(appendTranscript);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div
        data-testid="assistant-thread-body"
        ref={bodyRef}
        className="min-h-0 flex-1 overflow-y-auto px-(--space-6) py-(--space-6)"
      >
        {chat.loaded ? (
          chat.entries.length === 0 ? (
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
              {chat.entries.map((entry) =>
                entry.kind === "notice" ? (
                  <li key={entry.notice.id} data-seq={entry.seq} className="flex justify-start">
                    <NoticeBubble notice={entry.notice} />
                  </li>
                ) : (
                  <li
                    key={entry.message.id}
                    data-seq={entry.message.seq}
                    className={entry.message.role === "user" ? "flex justify-end" : "flex justify-start"}
                  >
                    <MessageBubble message={entry.message} />
                  </li>
                )
              )}
            </ol>
          )
        ) : (
          <Spinner label="Loading conversation" />
        )}
        <div ref={tailRef} aria-hidden="true" />
      </div>
      <div className="mx-auto w-full max-w-3xl shrink-0 px-(--space-6) pb-(--space-4)">
        {chat.error === null ? null : (
          <p role="alert" data-testid="assistant-error-line" className={`text-meta mb-(--space-2) ${ERROR_TEXT_CLASS}`}>
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
          dictation={dictation}
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

/** How far from the end still counts as "following the tail", in pixels. */
const TAIL_TOLERANCE_PX = 24;

/**
 * Whether the scrollport sits at its end. The tail marker leaves the list's
 * bottom padding visible, so anything within one line of the end counts.
 */
export function isFollowingTail(body: HTMLElement): boolean {
  return Math.abs(body.scrollHeight - body.scrollTop - body.clientHeight) < TAIL_TOLERANCE_PX;
}

/**
 * Keep the tail in view while content above it keeps growing. A reply grows
 * its row when the markdown renderer resolves, which happens after the
 * entries effect already scrolled; without this the newest row drifts below
 * the fold under load. Growth re-pins only while the reader is still at the
 * end — an explicit scroll up wins, and the watch re-arms when they come
 * back down. Growth is watched on the DOM, not the scrollport's box: the
 * port keeps its height and the list inside it is what grows. Returns a
 * disposer; hosts without a MutationObserver rely on the entries effect.
 */
export function followTailWhileGrowing(body: HTMLElement | null, tail: HTMLElement | null): () => void {
  if (body === null || tail === null || typeof MutationObserver === "undefined") return () => undefined;
  let pinned = true;
  const remember = (): void => {
    pinned = isFollowingTail(body);
  };
  const observer = new MutationObserver(() => {
    if (pinned) followTail(body, tail);
  });
  observer.observe(body, { childList: true, subtree: true, characterData: true });
  body.addEventListener("scroll", remember, { passive: true });
  return () => {
    observer.disconnect();
    body.removeEventListener("scroll", remember);
  };
}

/**
 * Assistant text as safe HTML. The renderer loads on demand, so a reply
 * arriving before it is ready renders once the promise lands; an empty or
 * non-assistant row never asks for it at all.
 */
function useMarkdownHtml(message: AssistantMessage): string | null {
  const wanted = message.role === "assistant" && message.content.trim() !== "";
  const [html, setHtml] = React.useState<string | null>(null);
  const source = message.content;

  React.useEffect(() => {
    if (!wanted) {
      setHtml(null);
      return;
    }
    let active = true;
    void markdownHtml(source).then((rendered) => {
      if (active) setHtml(rendered);
    });
    return () => {
      active = false;
    };
  }, [source, wanted]);

  return wanted ? html : null;
}

function MessageBubble({ message }: { message: AssistantMessage }): React.ReactElement {
  const html = useMarkdownHtml(message);

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
        className={`text-meta max-w-[80%] rounded-(--radius-popover) border border-(--danger) bg-(--surface) px-(--space-3) py-(--space-2) ${ERROR_TEXT_CLASS}`}
      >
        {message.content.trim() === "" ? "The assistant hit an error." : message.content}
      </div>
    );
  }
  if (message.role === "tool_call" || message.role === "tool_result") {
    const links = message.payload.links ?? [];
    return (
      <div
        data-role={message.role}
        data-testid="assistant-tool-activity"
        className="text-meta max-w-[80%] text-(--text-muted)"
      >
        <span data-testid="assistant-tool-text">{message.content}</span>
        {links.map((link) => (
          <a
            key={link.url_path}
            href={link.url_path}
            className="ml-(--space-2) font-medium text-(--accent) hover:underline"
          >
            {link.label ?? TOOL_LINK_FALLBACK}
          </a>
        ))}
      </div>
    );
  }
  return (
    <div
      data-role="assistant"
      className="text-body max-w-[80%] rounded-(--radius-popover) bg-(--subtle) px-(--space-3) py-(--space-2) text-(--text)"
    >
      {html === null ? (
        <span className="text-(--text-muted)">{EMPTY_REPLY_PLACEHOLDER}</span>
      ) : (
        <div className="rich-text-content" dangerouslySetInnerHTML={{ __html: html }} />
      )}
    </div>
  );
}

/**
 * One skipped-servers notice: a quiet inline row with one line per dropped
 * server, interleaved in the transcript — never a toast (AGT-046).
 */
function NoticeBubble({ notice }: { notice: ThreadNotice }): React.ReactElement {
  return (
    <div
      data-role="notice"
      data-testid="assistant-notice"
      className="text-meta flex max-w-[80%] flex-col gap-(--space-1) rounded-(--radius-popover) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-2) text-(--text-muted)"
    >
      {notice.servers.map((server) => (
        <span key={`${server.name}:${server.reason}`} data-testid="assistant-notice-line">
          {skippedServerLine(server)}
        </span>
      ))}
    </div>
  );
}
