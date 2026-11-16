// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One run in the workspace runs tab (NEWFRONT-545, RUN-017–021). The list
// and detail share one screen; this route supplies the selected id.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunsScreen } from "../../../../features/runners/runs/RunsScreen.js";

export const Route = createFileRoute("/$ws/_gated/runners/runs/$id")({
  component: RunsDetailRoute,
});

function RunsDetailRoute(): React.ReactElement {
  const { id } = Route.useParams();
  return <RunsScreen search={Route.useSearch()} selectedRunId={id} />;
}
