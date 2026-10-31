// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail route (WSET-022-024/036, NEWFRONT-541). The section gate
// is the webhooks gate by construction. The page owns the read states:
// spinner while it resolves, a missing-or-deleted explanation for an
// unknown id (the intended behavior behind NEWFRONT-417), and a retryable
// failure notice.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { WebhookDetailPage } from "../../../../../../features/workspace-settings/webhooks/WebhookDetailPage.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/webhooks/$webhookId")({
  component: WorkspaceWebhookDetailRoute,
});

function WorkspaceWebhookDetailRoute(): React.ReactElement {
  const { ws, webhookId } = Route.useParams();
  return <WebhookDetailPage workspaceSlug={ws} webhookId={webhookId} />;
}
