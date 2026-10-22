// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail, project scope (NEWFRONT-555, RUN-042/RUN-043). Same screen,
// project-scoped back link and chat address.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnerDetailContainer } from "../../../../../../../features/runners/detail/RunnerDetailContainer.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/detail/$runnerId")({
  component: ProjectRunnerDetailRoute,
});

function ProjectRunnerDetailRoute(): React.ReactElement {
  const { ws, projectId, runnerId } = Route.useParams();
  return <RunnerDetailContainer workspaceSlug={ws} runnerId={runnerId} projectId={projectId} />;
}
