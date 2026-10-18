// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs tab mount point, project scope (NEWFRONT-540, RUN-003). Owned by the
// slices that build the runs screens; the route keeps the tab deep-linkable.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { RunnersAreaSectionMount } from "../../../../../../features/runners/components/RunnersAreaSectionMount.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/runs")({
  component: RunsMountRoute,
});

function RunsMountRoute(): React.ReactElement {
  return <RunnersAreaSectionMount title="Runs" />;
}
