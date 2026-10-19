// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One run, project scope (NEWFRONT-545, RUN-017).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunsScreen } from "../../../../../../features/runners/runs/RunsScreen.js";
import { parseRunsPage, runsPageSearch } from "../../../../../../features/runners/runs/runsModel.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/runs_/$runId")({
  validateSearch: (search: Record<string, unknown>) => runsPageSearch(parseRunsPage(search)),
  component: RunDetailRouteComponent,
});

function RunDetailRouteComponent(): React.ReactElement {
  const { runId } = Route.useParams();
  const page = Route.useSearch().page ?? 1;
  return <RunsScreen page={page} selectedRunId={runId} />;
}
