// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs tab, workspace scope (NEWFRONT-545, RUN-016–022). The list lives at
// `/runs` with the page in `?page=`; the sibling `$runId` route selects a run.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunsScreen } from "../../../../features/runners/runs/RunsScreen.js";

export const Route = createFileRoute("/$ws/_gated/runners/runs/")({
  component: RunsIndexRoute,
});

function RunsIndexRoute(): React.ReactElement {
  return <RunsScreen search={Route.useSearch()} selectedRunId={null} />;
}
