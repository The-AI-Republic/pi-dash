// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant landing route (parity/specs/agents.md §9, AGT-038–AGT-040).
// Thin: the feature owns the greeting/setup swap and the start action.

import { Spinner } from "@pidash/kit";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { hasLocalChat } from "../../../../core/platform/local-chat.js";
import { AssistantLanding } from "../../../../features/assistant/components/AssistantLanding.js";

// Desktop chat (NEWPIDASH-300) splits off the initial bundle (size gate):
// the route tree statically imports this file on first paint, so a static
// import of the desktop surface would list its chunk in the route preload
// registry the entry ships. React.lazy keeps the desktop chunk out of that
// registry entirely: the call site (and its preload list) lives in this
// route chunk, which only loads on assistant navigation.
const DesktopAssistant = React.lazy(() =>
  import("./DesktopAssistant.js").then((module) => ({
    default: module.DesktopAssistant,
  }))
);

export const Route = createFileRoute("/$ws/_gated/assistant/")({
  component: AssistantLandingComponent,
});

function AssistantLandingComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  const navigate = useNavigate();
  // On the desktop this address is the local Pi Dash Agent chat; the
  // server landing below never mounts there (NEWPIDASH-300).
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
          requestedSessionId={null}
          onSelectSession={(sessionId) => {
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
  return <AssistantLanding key={ws} workspaceSlug={ws} />;
}
