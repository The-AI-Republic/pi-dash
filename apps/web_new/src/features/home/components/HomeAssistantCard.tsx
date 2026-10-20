// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home assistant card (SHELL-007 card shell + NEWFRONT-603 quick entry,
// parity/specs/agents.md §9 AGT-048 and §13). Members see a single-line
// draft with an Ask action, suggestion chips that fill the draft without
// sending, and up to five recent conversations newest-first. A lane with
// no credential is refused with a notice and starts nothing, and the card
// carries the setup reminder until an active lane is ready (CLOUD-037: a
// hosted or account lane counts, not only a self-supplied key). Guests
// never see the card.
// Recents link into the assistant view by id; asks start there.

import { Bot } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { Button, cn } from "@pidash/kit";
import type { AssistantThread } from "@pidash/api-client";

import { usePermissions } from "../../../core/session/index.js";
import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { notifyError } from "../../../shared/shell/index.js";
import { useAssistantConfig, useAssistantThreads } from "../api/queries.js";
import {
  ASSISTANT_CARD_SUGGESTIONS,
  API_KEY_REMINDER,
  API_KEY_REQUIRED_TITLE,
  assistantLandingHref,
  assistantThreadHref,
  conversationLabel,
  needsLaneSetup,
  startFailureDetail,
  START_FAILED_TITLE,
  useStartConversation,
} from "../../assistant/index.js";

/** How many conversations the card lists. */
export const ASSISTANT_CARD_RECENT_LIMIT = 5;

/** The newest unarchived conversations, capped for the card. */
export function recentConversations(threads: readonly AssistantThread[] | undefined): AssistantThread[] {
  return (threads ?? []).filter((thread) => !thread.is_archived).slice(0, ASSISTANT_CARD_RECENT_LIMIT);
}

export type CardAskOutcome = "blank" | "busy" | "keyless" | "start";

/**
 * What pressing Ask does for a draft (AGT-048): a blank draft does nothing, a
 * create already in flight absorbs repeats, a keyless member gets the
 * reminder instead of a start, and everything else starts a conversation.
 * The caller renders the notice or fires the start; this only decides.
 */
export function cardAskOutcome(content: string, configured: boolean, starting: boolean): CardAskOutcome {
  if (content.trim() === "") return "blank";
  if (starting) return "busy";
  return configured ? "start" : "keyless";
}

export interface HomeAssistantCardProps {
  workspaceSlug: string;
}

export function HomeAssistantCard({ workspaceSlug }: HomeAssistantCardProps): React.ReactElement | null {
  const permissions = usePermissions(workspaceSlug);
  const canView = !permissions.isLoading && permissions.canEdit;
  const threads = useAssistantThreads(workspaceSlug, canView);
  const config = useAssistantConfig(workspaceSlug, canView);
  const start = useStartConversation();
  const navigate = useNavigate();
  const [draft, setDraft] = React.useState("");

  if (!canView) return null;
  if (threads.data === undefined || config.data === undefined) return null;
  const configured = !needsLaneSetup(config.data);
  const recent = recentConversations(threads.data);

  const ask = (): void => {
    const content = draft.trim();
    const outcome = cardAskOutcome(content, configured, start.isPending);
    if (outcome === "blank" || outcome === "busy") return;
    if (outcome === "keyless") {
      notifyError(API_KEY_REQUIRED_TITLE, API_KEY_REMINDER);
      return;
    }
    start.mutate(
      { workspaceSlug },
      {
        onSuccess: (thread) => {
          setDraft("");
          void navigate({
            to: "/$ws/assistant/$threadId",
            params: { ws: workspaceSlug, threadId: thread.id },
            state: { pendingMessage: content },
          });
        },
        onError: (error) => notifyError(START_FAILED_TITLE, startFailureDetail(error)),
      }
    );
  };

  return (
    <section
      aria-label="Assistant"
      id="home-assistant-card"
      data-assistant-state={configured ? "ready" : "setup"}
      className="flex flex-col gap-(--space-4) rounded-(--radius-dialog) border border-(--border) bg-(--surface) p-(--space-6)"
    >
      <h2 className="text-h3 flex items-center gap-(--space-2) text-(--text)">
        <Bot size={18} aria-hidden="true" />
        Assistant
      </h2>
      {configured ? null : (
        <div className="flex flex-wrap items-center gap-(--space-2)">
          <p className="text-body flex-1 text-(--text-muted)">
            Connect a model to start delegating work to the assistant.
          </p>
          <CrossAppLink
            to={assistantLandingHref(workspaceSlug)}
            className="text-body font-medium text-(--accent) hover:underline"
          >
            Set up the assistant
          </CrossAppLink>
        </div>
      )}
      <div data-testid="assistant-card-entry" className="flex items-end gap-(--space-2)">
        <input
          aria-label="Ask the assistant"
          placeholder="Ask Pi Dash to do something…"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            // The single-line entry submits on unmodified Enter only; a chord
            // (Control/Command/Alt) never asks (AGT-049 intended).
            if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
              event.preventDefault();
              ask();
            }
          }}
          className={cn(
            "text-body h-(--control-height) min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4)",
            "text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
          )}
        />
        <Button
          variant="primary"
          onClick={ask}
          disabled={draft.trim() === "" || start.isPending}
          loading={start.isPending}
        >
          Ask
        </Button>
      </div>
      <div className="flex flex-wrap gap-(--space-2)">
        {ASSISTANT_CARD_SUGGESTIONS.map((suggestion) => (
          <button
            key={suggestion}
            type="button"
            data-suggestion
            onClick={() => setDraft(suggestion)}
            className="text-body cursor-pointer rounded-full border border-(--border) bg-(--bg) px-(--space-4) py-(--space-1) text-(--text) hover:bg-(--subtle)"
          >
            {suggestion}
          </button>
        ))}
      </div>
      {recent.length > 0 ? (
        <div className="flex flex-col gap-(--space-2)">
          <h3 className="text-emphasis text-(--text-muted)">Recent conversations</h3>
          <ul className="flex flex-col gap-(--space-1)">
            {recent.map((thread) => (
              <li key={thread.id}>
                <CrossAppLink
                  to={assistantThreadHref(workspaceSlug, thread.id)}
                  className="text-body block truncate text-(--text) hover:underline"
                >
                  {conversationLabel(thread)}
                </CrossAppLink>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}
