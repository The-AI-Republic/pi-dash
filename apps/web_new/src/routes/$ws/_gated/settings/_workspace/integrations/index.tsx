// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations section route (WSET-016–018). Thin: the settings layout
// owns the frame, the admin gate and the tab title; the integrations
// feature owns everything inside them.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { IntegrationsPage } from "../../../../../../features/workspace-settings/integrations/IntegrationsPage.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/integrations/")({
  component: WorkspaceSettingsIntegrationsPage,
});

function WorkspaceSettingsIntegrationsPage(): React.ReactElement {
  const { ws } = Route.useParams();
  return <IntegrationsPage workspaceSlug={ws} />;
}
