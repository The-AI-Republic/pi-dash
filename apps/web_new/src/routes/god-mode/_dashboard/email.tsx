// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// God-mode email route (ADM-018–022, from parity/specs/admin.md §3): the
// SMTP settings page mounting in the dashboard layout at
// /god-mode/email/.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminEmailPage } from "../../../features/admin/components/AdminEmailPage.js";

export const Route = createFileRoute("/god-mode/_dashboard/email")({
  component: EmailRouteComponent,
});

function EmailRouteComponent(): React.ReactElement {
  return <AdminEmailPage />;
}
