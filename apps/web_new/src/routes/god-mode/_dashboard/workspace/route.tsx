// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace layout route (from parity/specs/admin.md §5): the bare
// outlet the list index and the create page render through. Guard plus
// shell belong to the dashboard layout above it.
import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

export const Route = createFileRoute("/god-mode/_dashboard/workspace")({
  component: WorkspaceLayoutRoute,
});

function WorkspaceLayoutRoute(): React.ReactElement {
  return <Outlet />;
}
