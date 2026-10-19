// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs tab, project scope (NEWFRONT-545, RUN-016–022). Same screen as the
// workspace twin, scoped by the route's project id. The optional run
// parameter serves both the list and one directly linkable run.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunsScreen } from "../../../../../../features/runners/runs/RunsScreen.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/runs/{-$runId}")({
  component: RunsScreenRoute,
});

function RunsScreenRoute(): React.ReactElement {
  const { runId } = Route.useParams();
  return <RunsScreen search={Route.useSearch()} selectedRunId={runId ?? null} />;
}
