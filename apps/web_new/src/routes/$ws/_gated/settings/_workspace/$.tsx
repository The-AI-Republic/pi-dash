// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unknown workspace-settings address (WSET-002/036). The layout resolves
// no section and renders the not-authorized surface on the requested URL
// instead of a sidebar with a dead entry; this leaf only keeps the
// address out of the workspace catch-all. It never renders inside the
// layout's outlet, so it has no body of its own.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/$")({
  component: UnknownSettingsSection,
});

function UnknownSettingsSection(): React.ReactElement {
  return <React.Fragment />;
}
