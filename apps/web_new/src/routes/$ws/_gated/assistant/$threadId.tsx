// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation route (parity/specs/agents.md §9; internals owned by
// NEWFRONT-605). This slice mounts the conversation gate — the setup
// empty state, the locked/unlocked composer and the one-shot first-message
// handoff — and leaves transcript streaming, stop and paging to the
// thread-view slice, which fills the gate's transcript slot.

import { Spinner } from "@pidash/kit";
import { createFileRoute, useNavigate, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { hasLocalChat } from "../../../../core/platform/local-chat.js";
import { readPendingMessage } from "../../../../features/assistant/lib/navigation.js";
import { ConversationGate } from "../../../../features/assistant/components/ConversationGate.js";

// Desktop chat (NEWPIDASH-300) splits off the initial bundle (size gate);
// see the landing route for why the desktop surface stays lazy.
const DesktopAssistant = React.lazy(() =>
  import("./DesktopAssistant.js").then((module) => ({
    default: module.DesktopAssistant,
  }))
);

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
      <React.Suspense
        fallback={
          <div className="flex h-full items-center justify-center">
            <Spinner label="Loading assistant" />
          </div>
        }
      >
        <DesktopAssistant
          key={ws}
          workspaceSlug={ws}
          requestedSessionId={threadId}
          initialMessage={readPendingMessage(state)}
          onInitialMessageConsumed={() => {
            void navigate({ href: "", replace: true, state: {} });
          }}
          onSelectSession={(sessionId) => {
            if (sessionId === threadId) return;
            void navigate({
              to: "/$ws/assistant/$threadId",
              params: { ws, threadId: sessionId },
              replace: true,
            });
          }}
        />
      </React.Suspense>
    );
  }
  return <ConversationGate key={threadId} workspaceSlug={ws} threadId={threadId} state={state} />;
}
