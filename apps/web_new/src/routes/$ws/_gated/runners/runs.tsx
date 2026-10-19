// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs tab, workspace scope (NEWFRONT-545, RUN-016–022). The page number
// lives in `?page=`; the detail pane opens through the sibling route so a
// run is directly linkable.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunsScreen } from "../../../../features/runners/runs/RunsScreen.js";
import { parseRunsPage, runsPageSearch } from "../../../../features/runners/runs/runsModel.js";

export const Route = createFileRoute("/$ws/_gated/runners/runs")({
  validateSearch: (search: Record<string, unknown>) => runsPageSearch(parseRunsPage(search)),
  component: RunsRouteComponent,
});

function RunsRouteComponent(): React.ReactElement {
  const page = Route.useSearch().page ?? 1;
  return <RunsScreen page={page} selectedRunId={null} />;
}
