// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant landing route (parity/specs/agents.md §9, AGT-038–AGT-040).
// Thin: the feature owns the greeting/setup swap and the start action.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { AssistantLanding } from "../../../../features/assistant/components/AssistantLanding.js";
import { DesktopAssistant } from "../../../../features/assistant/components/DesktopAssistant.js";
import { hasLocalChat } from "../../../../features/runners/index.js";

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
    );
  }
  return <AssistantLanding key={ws} workspaceSlug={ws} />;
}
