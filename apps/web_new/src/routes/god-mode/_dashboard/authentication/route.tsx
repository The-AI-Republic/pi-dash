// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Authentication layout route (from parity/specs/admin.md §4): the bare
// outlet the landing index and the four provider sub-pages render
// through. Guard plus shell belong to the dashboard layout above it.
import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

export const Route = createFileRoute("/god-mode/_dashboard/authentication")({
  component: AuthenticationLayoutRoute,
});

function AuthenticationLayoutRoute(): React.ReactElement {
  return <Outlet />;
}
