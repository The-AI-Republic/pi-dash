// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Billing section route (WSET-001/029, NEWFRONT-546). Thin: the settings
// layout owns the frame, the admin gate and the tab title; the billing
// feature owns everything inside them.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { BillingPage } from "../../../../../../features/workspace-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/billing/")({
  component: WorkspaceSettingsSectionPage,
});

function WorkspaceSettingsSectionPage(): React.ReactElement {
  const { ws } = Route.useParams();
  return <BillingPage workspaceSlug={ws} />;
}
