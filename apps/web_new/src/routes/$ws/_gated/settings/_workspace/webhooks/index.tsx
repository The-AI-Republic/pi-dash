// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhooks section route (WSET-001/019, NEWFRONT-541). Thin: the settings
// layout owns the frame, the admin gate and the tab title; the webhooks
// feature owns the list, the creation dialog and the row switch.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { WebhooksListPage } from "../../../../../../features/workspace-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/webhooks/")({
  component: WorkspaceWebhooksPage,
});

function WorkspaceWebhooksPage(): React.ReactElement {
  const { ws } = Route.useParams();
  return <WebhooksListPage workspaceSlug={ws} />;
}
