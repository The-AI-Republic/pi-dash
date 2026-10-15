// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// God-mode layout route (ADM-072, from parity/specs/admin.md §0.2):
// the web-only mount. Desktop builds never mount the console: the
// gate throws not-found before any console route loads, so every
// /god-mode/* address renders the app not-found surface there.
// Children: the entry gate, the dashboard layout, and the unguarded
// catch-all.
import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { platform } from "../../core/platform/index.js";

export const Route = createFileRoute("/god-mode")({
  beforeLoad: () => {
    if (platform.kind === "desktop") throw notFound();
    return {};
  },
  component: GodModeLayoutComponent,
});

function GodModeLayoutComponent(): React.ReactElement {
  return <Outlet />;
}
