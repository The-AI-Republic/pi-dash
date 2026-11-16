// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation route (parity/specs/agents.md §9; internals owned by
// NEWFRONT-605). This slice mounts the conversation gate — the setup
// empty state, the locked/unlocked composer and the one-shot first-message
// handoff — and leaves transcript streaming, stop and paging to the
// thread-view slice, which fills the gate's transcript slot.

import { createFileRoute, useNavigate, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { ConversationGate } from "../../../../features/assistant/components/ConversationGate.js";
import { DesktopAssistant } from "../../../../features/assistant/components/DesktopAssistant.js";
import { hasLocalChat } from "../../../../features/runners/index.js";

export const Route = createFileRoute("/$ws/_gated/assistant/$threadId")({
  component: ConversationComponent,
});

function ConversationComponent(): React.ReactElement {
  const { ws, threadId } = Route.useParams();
  const state = useRouterState({ select: (current) => current.location.state });
  const navigate = useNavigate();
  // On the desktop this address names a local session of the Pi Dash
  // Agent; the server conversation below never mounts there (NEWPIDASH-300).
  if (hasLocalChat()) {
    return (
      <DesktopAssistant
        key={ws}
        workspaceSlug={ws}
        requestedSessionId={threadId}
        onSelectSession={(sessionId) => {
          if (sessionId === threadId) return;
          void navigate({
            to: "/$ws/assistant/$threadId",
            params: { ws, threadId: sessionId },
            replace: true,
          });
        }}
      />
    );
  }
  return <ConversationGate key={threadId} workspaceSlug={ws} threadId={threadId} state={state} />;
}
