// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard layout route (pathless): every settings page mounts here
// and inherits the guard, the shell, and the first-workspace popup.
// Page slices add their routes as files under _dashboard/ (a page at
// _dashboard/general.tsx serves /god-mode/general/). The entry gate
// and the catch-all stay direct children of /god-mode, outside this
// layout.
import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { AdminDashboardLayout } from "../../../features/admin/components/AdminDashboardLayout.js";

export const Route = createFileRoute("/god-mode/_dashboard")({
  component: DashboardLayoutRoute,
});

function DashboardLayoutRoute(): React.ReactElement {
  return (
    <AdminDashboardLayout>
      <Outlet />
    </AdminDashboardLayout>
  );
}
