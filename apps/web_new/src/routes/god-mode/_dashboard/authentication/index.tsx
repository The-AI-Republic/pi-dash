// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Authentication landing route (ADM-023–026, ADM-036, from
// parity/specs/admin.md §4): the open-sign-up switch plus the method
// registry inside the dashboard guard and shell. The index child of the
// authentication layout; provider sub-pages are its siblings.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminAuthLanding } from "../../../../features/admin/components/AdminAuthLanding.js";

export const Route = createFileRoute("/god-mode/_dashboard/authentication/")({
  component: AuthenticationRoute,
});

function AuthenticationRoute(): React.ReactElement {
  return <AdminAuthLanding />;
}
