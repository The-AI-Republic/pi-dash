// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace settings exports route (parity/specs/workspace-settings.md
// §33, WSET-033). The shell owns the frame and the gate; this route mounts
// the exports entry, which in turn offers the exports area its guide slot.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ExportsSettingsPage } from "../../../../../../features/workspace-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/exports/")({
  component: WorkspaceSettingsSectionPage,
});

function WorkspaceSettingsSectionPage(): React.ReactElement {
  const { ws } = Route.useParams();
  return <ExportsSettingsPage workspaceSlug={ws} />;
}
