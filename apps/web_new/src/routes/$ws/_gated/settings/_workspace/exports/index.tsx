// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace settings exports route (parity/specs/workspace-settings.md
// §33, WSET-033). The shell owns the frame and the gate; this slice
// registers the exports entry in the shell's page registry, and the route
// only mounts the section outlet. That keeps one entry point for the
// exports area and its guide slot.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ExportsSettingsPage } from "../../../../../../features/workspace-settings/components/ExportsSettingsPage.js";
import { registerWorkspaceSettingsPage } from "../../../../../../features/workspace-settings/lib/registry.js";
import { SettingsSectionOutlet } from "../../../../../../features/workspace-settings/components/SettingsSectionOutlet.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/exports/")({
  component: WorkspaceSettingsSectionPage,
});

function WorkspaceSettingsSectionPage(): React.ReactElement {
  const { ws } = Route.useParams();
  // Register for as long as this section route is mounted: the shell outlet
  // stays the only render path, and the exports area's page has exactly one
  // entry point.
  React.useLayoutEffect(() => registerWorkspaceSettingsPage("exports", ExportsSettingsPage), []);
  // The guide chunk registers itself into the frame's seam on import, so
  // the load starts from the route body and the form + history stay out of
  // the initial bundle (NEWFRONT-768).
  React.useEffect(() => {
    void import("../../../../../../features/exports/register.js");
  }, []);
  return <SettingsSectionOutlet workspaceSlug={ws} section="exports" />;
}
