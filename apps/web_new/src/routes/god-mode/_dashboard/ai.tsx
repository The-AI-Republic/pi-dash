// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI settings route (ADM-046–048, from parity/specs/admin.md §6):
// mounts the page inside the dashboard layout's guard plus shell.
// There is deliberately no sibling image route: /god-mode/image/
// falls through to the catch-all not-found screen (ADM-049/050,
// removal target per main#523).
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { AdminAiSettingsPage } from "../../../features/admin/components/AdminAiSettingsPage.js";

export const Route = createFileRoute("/god-mode/_dashboard/ai")({
  component: AiSettingsRouteComponent,
});

function AiSettingsRouteComponent(): React.ReactElement {
  return <AdminAiSettingsPage />;
}
