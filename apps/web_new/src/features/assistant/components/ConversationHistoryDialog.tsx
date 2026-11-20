// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation history dialog (parity/specs/agents.md §11, AGT-044/AGT-061).
// The assistant's own history list — link rows, never the shared
// button-based history panel. Each cached conversation links to its own
// address newest-first with an untitled fallback, the open one carries
// `aria-current`, archived rows never render, and a resolved-but-empty list
// shows the placeholder. A refused read (guests) leaves the list
// unresolved, so neither rows nor the placeholder appear — the exclusion
// AGT-041 pins. The dialog has no buttons of its own: picking a
// conversation navigates to it and closes the dialog, while Escape and the
// backdrop also close it, and the dialog traps focus while open. Opening
// moves focus to the first row once the read settles (the list itself when
// empty), because the rows mount after the read and the opener would
// otherwise keep focus.

import { Link, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { Dialog } from "@pidash/kit";

import { useAssistantThreads } from "../api/queries.js";
import { CHAT_HISTORY_TITLE, NO_CONVERSATIONS } from "../lib/copy.js";
import { conversationLabel, visibleConversations } from "../lib/labels.js";
import { assistantThreadHref } from "../lib/navigation.js";

export interface ConversationHistoryDialogProps {
  workspaceSlug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ConversationHistoryDialog({
  workspaceSlug,
  open,
  onOpenChange,
}: ConversationHistoryDialogProps): React.ReactElement {
  const threads = useAssistantThreads(workspaceSlug);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const rows = visibleConversations(threads.data);
  const empty = threads.isSuccess && rows.length === 0;

  // The rows mount after the thread read settles — and the portal mounts
  // the list asynchronously on open — so the dialog takes over focusing
  // entirely through the list's own attach: once the read settles, move
  // focus inside when it is still outside (the opener keeps it otherwise,
  // and the first Tab would escape to the page header). The effect-free
  // callback refires on every attach, including the settle flip that
  // arrives after the portal mount. The nav is the fallback when there is
  // no row to land on; it is programmatically focusable but never a tab
  // stop.
  const focusWhenReady = React.useCallback(
    (node: HTMLElement | null) => {
      if (!node || !open || (!threads.isSuccess && !threads.isError)) return;
      const active = document.activeElement;
      if (active && node.contains(active)) return;
      const first = node.querySelector("a");
      (first ?? node).focus();
    },
    [open, threads.isSuccess, threads.isError]
  );

  return (
    <Dialog title={CHAT_HISTORY_TITLE} open={open} onOpenChange={onOpenChange} closeButton={false} initialFocus={false}>
      <div data-testid="assistant-history-dialog" className="flex min-h-0 flex-col">
        <nav
          ref={focusWhenReady}
          tabIndex={-1}
          aria-label={CHAT_HISTORY_TITLE}
          className="flex min-h-0 flex-1 flex-col gap-(--space-1) overflow-y-auto"
        >
          {rows.map((thread) => (
            <Link
              key={thread.id}
              to="/$ws/assistant/$threadId"
              params={{ ws: workspaceSlug, threadId: thread.id }}
              onClick={() => onOpenChange(false)}
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
      </div>
    </Dialog>
  );
}
