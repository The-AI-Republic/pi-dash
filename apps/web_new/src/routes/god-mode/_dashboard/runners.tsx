// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners list route (ADM-080, from parity/specs/admin.md §11): the
// runners page mounts here and inherits the guard, the shell, and
// the first-workspace popup from the dashboard layout.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminRunnersPage } from "../../../features/admin/index.js";

export const Route = createFileRoute("/god-mode/_dashboard/runners")({
  component: RunnersRouteComponent,
});

function RunnersRouteComponent(): React.ReactElement {
  return <AdminRunnersPage />;
}
