// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs tab, workspace scope (NEWFRONT-545, RUN-016–022). One optional
// parameter serves the list and a directly linkable run: `/runs` leaves
// the detail empty, `/runs/<id>` selects it. The page number lives in
// `?page=`.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunsScreen } from "../../../../features/runners/runs/RunsScreen.js";

export const Route = createFileRoute("/$ws/_gated/runners/runs/{-$runId}")({
  component: RunsScreenRoute,
});

function RunsScreenRoute(): React.ReactElement {
  const { runId } = Route.useParams();
  return <RunsScreen search={Route.useSearch()} selectedRunId={runId ?? null} />;
}
