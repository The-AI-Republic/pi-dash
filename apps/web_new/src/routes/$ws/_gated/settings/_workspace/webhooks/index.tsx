// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace settings section route (WSET-001/036). Thin: the shell owns
// the frame and the gate, and the webhooks page slice registers its body in the
// shared page registry.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { SettingsSectionOutlet } from "../../../../../../features/workspace-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/webhooks/")({
  component: WorkspaceSettingsSectionPage,
});

function WorkspaceSettingsSectionPage(): React.ReactElement {
  const { ws } = Route.useParams();
  return <SettingsSectionOutlet workspaceSlug={ws} section="webhooks" />;
}
