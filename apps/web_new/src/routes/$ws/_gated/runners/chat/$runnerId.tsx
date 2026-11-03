// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat, workspace scope (NEWFRONT-549, RUN-025/026). The runner id
// is the address, so every rail contact deep-links here; `?sessionId=`
// names a conversation. Selecting an entry rewrites that parameter
// without adding history, matching the old surface.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnerChatPage } from "../../../../../features/runners/chat/RunnerChatPage.js";

const Route = createFileRoute("/$ws/_gated/runners/chat/$runnerId")({
  validateSearch: (search: Record<string, unknown>): { sessionId?: string } => {
    const sessionId = search["sessionId"];
    return typeof sessionId === "string" && sessionId !== "" ? { sessionId } : {};
  },
  component: RunnerChatRouteComponent,
});

function RunnerChatRouteComponent(): React.ReactElement {
  const { ws, runnerId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <RunnerChatPage
      workspaceSlug={ws}
      runnerId={runnerId}
      requestedSessionId={search.sessionId ?? null}
      onSelectSession={(sessionId) =>
        void navigate({
          to: "/$ws/runners/chat/$runnerId",
          params: { ws, runnerId },
          search: { sessionId },
          replace: true,
        })
      }
    />
  );
}

export { Route };
