// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail address (WSET-022/036). The section gate is the
// webhooks gate by construction; this route only hands the id to the
// shell's webhook-detail outlet, which holds the loading, missing and
// registered-body states.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { SettingsWebhookDetailOutlet } from "../../../../../../features/workspace-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/webhooks/$webhookId")({
  component: WorkspaceWebhookDetailPage,
});

function WorkspaceWebhookDetailPage(): React.ReactElement {
  const { ws, webhookId } = Route.useParams();
  return <SettingsWebhookDetailOutlet workspaceSlug={ws} webhookId={webhookId} />;
}
