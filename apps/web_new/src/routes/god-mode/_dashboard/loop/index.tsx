// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loop jobs list route (ADM-051–054, from parity/specs/admin.md
// §7.1–§7.4): the jobs table mounts here and inherits the guard, the
// shell, and the first-workspace popup from the dashboard layout.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminLoopJobsPage } from "../../../../features/admin/components/AdminLoopJobsPage.js";

export const Route = createFileRoute("/god-mode/_dashboard/loop/")({
  component: LoopJobsRouteComponent,
});

function LoopJobsRouteComponent(): React.ReactElement {
  return <AdminLoopJobsPage />;
}
