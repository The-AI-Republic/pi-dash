// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home assistant card (SHELL-007; written from parity/specs/shell.md §2
// and the §10 seam). Members see suggested prompts plus recent threads
// when a model is configured, or a setup reminder when it is not; guests
// see no card at all. Threads link into the assistant view by id while
// chips and the reminder lead to its landing — the assistant area owns
// everything past those links.

import { Bot } from "lucide-react";
import * as React from "react";

import { usePermissions } from "../../../core/session/index.js";
import { CrossAppLink } from "../../../shared/coexistence/index.js";
import { useAssistantConfig, useAssistantThreads } from "../api/queries.js";
import {
  ASSISTANT_SUGGESTIONS,
  assistantLandingHref,
  assistantThreadHref,
  isAssistantConfigured,
} from "../lib/assistant.js";

export interface HomeAssistantCardProps {
  workspaceSlug: string;
}

export function HomeAssistantCard({ workspaceSlug }: HomeAssistantCardProps): React.ReactElement | null {
  const permissions = usePermissions(workspaceSlug);
  const canView = !permissions.isLoading && permissions.canEdit;
  const threads = useAssistantThreads(workspaceSlug, canView);
  const config = useAssistantConfig(workspaceSlug, canView);
  if (!canView) return null;
  if (threads.data === undefined || config.data === undefined) return null;
  const configured = isAssistantConfigured(config.data);
  const recent = threads.data.filter((thread) => !thread.is_archived);
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
      {configured ? (
        <>
          <div className="flex flex-wrap gap-(--space-2)">
            {ASSISTANT_SUGGESTIONS.map((suggestion) => (
              <CrossAppLink
                key={suggestion}
                to={assistantLandingHref(workspaceSlug)}
                data-suggestion
                className="text-body rounded-full border border-(--border) bg-(--bg) px-(--space-4) py-(--space-2) text-(--text) hover:bg-(--subtle)"
              >
                {suggestion}
              </CrossAppLink>
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
                      className="text-body text-(--text) hover:underline"
                    >
                      {thread.title === "" ? "Untitled conversation" : thread.title}
                    </CrossAppLink>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </>
      ) : (
        <>
          <p className="text-body text-(--text-muted)">Connect a model to start delegating work to the assistant.</p>
          <CrossAppLink
            to={assistantLandingHref(workspaceSlug)}
            className="text-body font-medium text-(--accent) hover:underline"
          >
            Set up the assistant
          </CrossAppLink>
        </>
      )}
    </section>
  );
}
