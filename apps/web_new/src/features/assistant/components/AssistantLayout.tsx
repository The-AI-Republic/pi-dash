// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant layout (parity/specs/agents.md §9/§11). The conversation uses
// the full width: a page header holds the Chat History button and the New
// chat entry, and the history dialog lists the cached conversations. The
// history read is the only shared state between the landing and a
// conversation. On the desktop the header is gone too: the routed content
// is the local chat with its own history column (NEWPIDASH-300).

import { Link, Outlet } from "@tanstack/react-router";
import { History } from "lucide-react";
import * as React from "react";

import { Button } from "@pidash/kit";

import { hasLocalChat } from "../../../core/platform/local-chat.js";
import { CHAT_HISTORY_TITLE, NEW_CONVERSATION_LABEL } from "../lib/copy.js";
import { ConversationHistoryDialog } from "./ConversationHistoryDialog.js";

export interface AssistantLayoutProps {
  workspaceSlug: string;
}

export function AssistantLayout({ workspaceSlug }: AssistantLayoutProps): React.ReactElement {
  // On the desktop the routed content is the local chat, whose own
  // history column carries the conversations; the server history would list
  // cloud threads the desktop page never opens (NEWPIDASH-300).
  const local = hasLocalChat();
  const [historyOpen, setHistoryOpen] = React.useState(false);
  if (local) {
    return (
      <div data-testid="assistant-layout" className="flex h-full w-full overflow-hidden">
        <main className="min-w-0 flex-1 overflow-hidden">
          <Outlet />
        </main>
      </div>
    );
  }
  return (
    <div data-testid="assistant-layout" className="flex h-full w-full flex-col overflow-hidden">
      <header className="flex h-12 shrink-0 items-center gap-(--space-2) border-b border-(--border) bg-(--surface) px-(--space-3)">
        <Button variant="ghost" size="small" onClick={() => setHistoryOpen(true)}>
          <History size={16} strokeWidth={1.5} aria-hidden="true" />
          <span>{CHAT_HISTORY_TITLE}</span>
        </Button>
        <Link
          to="/$ws/assistant"
          params={{ ws: workspaceSlug }}
          className="text-body flex h-9 items-center rounded-(--radius-control) px-(--space-3) font-medium text-(--text) hover:bg-(--subtle)"
          activeOptions={{ exact: true }}
          activeProps={{ className: "bg-(--subtle)" }}
        >
          {NEW_CONVERSATION_LABEL}
        </Link>
      </header>
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <main className="min-w-0 flex-1 overflow-hidden">
          <Outlet />
        </main>
      </div>
      <ConversationHistoryDialog workspaceSlug={workspaceSlug} open={historyOpen} onOpenChange={setHistoryOpen} />
    </div>
  );
}
