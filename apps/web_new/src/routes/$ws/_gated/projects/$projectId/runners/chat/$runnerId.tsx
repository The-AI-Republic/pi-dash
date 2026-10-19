// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner chat, project scope (NEWFRONT-549, RUN-025/026). The project twin
// of the workspace chat route: the same page inside the project chrome,
// with every link and read scoped by the route's project id.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnerChatPage } from "../../../../../../../features/runners/chat/RunnerChatPage.js";

const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/chat/$runnerId")({
  validateSearch: (search: Record<string, unknown>): { sessionId?: string } => {
    const sessionId = search["sessionId"];
    return typeof sessionId === "string" && sessionId !== "" ? { sessionId } : {};
  },
  component: ProjectRunnerChatRouteComponent,
});

function ProjectRunnerChatRouteComponent(): React.ReactElement {
  const { ws, projectId, runnerId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  return (
    <RunnerChatPage
      workspaceSlug={ws}
      runnerId={runnerId}
      requestedSessionId={search.sessionId ?? null}
      onSelectSession={(sessionId) =>
        void navigate({
          to: "/$ws/projects/$projectId/runners/chat/$runnerId",
          params: { ws, projectId, runnerId },
          search: { sessionId },
          replace: true,
        })
      }
    />
  );
}

export { Route };
