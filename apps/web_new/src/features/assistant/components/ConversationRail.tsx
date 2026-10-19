// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation rail (parity/specs/agents.md §11, AGT-044). Each cached
// conversation links to its own address newest-first with an untitled
// fallback, the open one carries `aria-current`, archived rows never
// render, and a resolved-but-empty list shows the placeholder. A refused
// read (guests) leaves the list unresolved, so neither rows nor the
// placeholder appear — the exclusion AGT-041 pins.

import { Link, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { useAssistantThreads } from "../api/queries.js";
import { CONVERSATION_RAIL_TITLE, NEW_CONVERSATION_LABEL, NO_CONVERSATIONS } from "../lib/copy.js";
import { conversationLabel, visibleConversations } from "../lib/labels.js";
import { assistantThreadHref } from "../lib/navigation.js";

export interface ConversationRailProps {
  workspaceSlug: string;
}

export function ConversationRail({ workspaceSlug }: ConversationRailProps): React.ReactElement {
  const threads = useAssistantThreads(workspaceSlug);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const rows = visibleConversations(threads.data);
  const empty = threads.isSuccess && rows.length === 0;

  return (
    <aside
      data-testid="assistant-rail"
      className="flex w-[280px] shrink-0 flex-col gap-(--space-3) border-r border-(--border) bg-(--surface) p-(--space-3)"
    >
      <Link
        to="/$ws/assistant"
        params={{ ws: workspaceSlug }}
        className="text-body flex h-9 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) font-medium text-(--text) hover:bg-(--subtle)"
        activeOptions={{ exact: true }}
        activeProps={{ "aria-current": "page" }}
      >
        {NEW_CONVERSATION_LABEL}
      </Link>
      <h2 className="text-meta px-(--space-2) font-medium text-(--text-muted) uppercase">{CONVERSATION_RAIL_TITLE}</h2>
      <nav
        aria-label={CONVERSATION_RAIL_TITLE}
        className="flex min-h-0 flex-1 flex-col gap-(--space-1) overflow-y-auto"
      >
        {rows.map((thread) => (
          <Link
            key={thread.id}
            to="/$ws/assistant/$threadId"
            params={{ ws: workspaceSlug, threadId: thread.id }}
            className="text-body min-h-9 rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:bg-(--subtle)"
            activeProps={{ "aria-current": "page", className: "bg-(--subtle) font-medium text-(--text)" }}
            data-thread-id={thread.id}
            data-active={pathname === assistantThreadHref(workspaceSlug, thread.id) ? "true" : "false"}
          >
            {conversationLabel(thread)}
          </Link>
        ))}
        {empty ? (
          <p className="text-meta px-(--space-3) py-(--space-2) text-(--text-muted)">{NO_CONVERSATIONS}</p>
        ) : null}
      </nav>
    </aside>
  );
}
