// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General settings route (ADM-013–017, from parity/specs/admin.md
// §2): the page mounts in the dashboard layout and inherits its
// guard, shell, and first-workspace popup.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminGeneralSettingsPage } from "../../../features/admin/components/AdminGeneralSettingsPage.js";

export const Route = createFileRoute("/god-mode/_dashboard/general")({
  component: GeneralSettingsRoute,
});

function GeneralSettingsRoute(): React.ReactElement {
  return <AdminGeneralSettingsPage />;
}
