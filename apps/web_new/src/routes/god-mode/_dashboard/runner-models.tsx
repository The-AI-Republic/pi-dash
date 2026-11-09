// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner models route (NEWPIDASH-169, DIFF-8): the catalog page mounts
// here and inherits the guard, the shell, and the first-workspace
// popup from the dashboard layout.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminRunnerModelsPage } from "../../../features/admin/components/AdminRunnerModelsPage.js";

export const Route = createFileRoute("/god-mode/_dashboard/runner-models")({
  component: RunnerModelsRouteComponent,
});

function RunnerModelsRouteComponent(): React.ReactElement {
  return <AdminRunnerModelsPage />;
}
