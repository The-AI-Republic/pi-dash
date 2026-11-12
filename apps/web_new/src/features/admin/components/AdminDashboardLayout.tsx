// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard layout (ADM-062–068, from parity/specs/admin.md §8): the
// shell every settings page mounts in. The dashboard guard wraps the
// tree (loader while unresolved, replace-navigation out for anyone
// but a signed-in admin), the stored theme applies on mount so a
// reload keeps it, the toast host renders page notices (ADM-023 auth
// saves, ADM-052 loop toggle failures), and the first-workspace popup
// rides above the page content. Dashboard pages reach this through
// the `_dashboard` pathless route, never around it.
import { ToastHost } from "@pidash/kit";
import * as React from "react";

import { getToastManager } from "../../../shared/shell/index.js";
import { applyStoredAdminTheme } from "../theme.js";
import { AdminDashboardGuard } from "./AdminGuards.js";
import { AdminHeader } from "./AdminHeader.js";
import { AdminSidebar } from "./AdminSidebar.js";
import { FirstWorkspacePopup } from "./FirstWorkspacePopup.js";

export function AdminDashboardLayout({ children }: { children: React.ReactNode }): React.ReactElement {
  React.useEffect(() => {
    applyStoredAdminTheme();
  }, []);
  return (
    <AdminDashboardGuard>
      <div data-testid="admin-dashboard" className="relative flex h-screen w-screen overflow-hidden">
        <AdminSidebar />
        <main data-testid="admin-main" className="relative flex h-full w-full flex-col overflow-hidden bg-(--surface)">
          <AdminHeader />
          <div data-testid="admin-content" className="h-full w-full overflow-y-auto">
            {children}
          </div>
        </main>
        <FirstWorkspacePopup />
        <ToastHost manager={getToastManager()} />
      </div>
    </AdminDashboardGuard>
  );
}
