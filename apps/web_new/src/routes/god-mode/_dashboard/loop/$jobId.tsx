// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop job detail route (ADM-058–061/073, from
// parity/specs/admin.md §7.8–§7.11): the job page mounts here with
// the path's job id and inherits the guard, the shell, and the
// first-workspace popup from the dashboard layout.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminLoopJobDetailPage } from "../../../../features/admin/components/AdminLoopJobDetailPage.js";

export const Route = createFileRoute("/god-mode/_dashboard/loop/$jobId")({
  component: LoopJobDetailRouteComponent,
});

function LoopJobDetailRouteComponent(): React.ReactElement {
  const params = Route.useParams();
  return <AdminLoopJobDetailPage jobId={params.jobId} />;
}
