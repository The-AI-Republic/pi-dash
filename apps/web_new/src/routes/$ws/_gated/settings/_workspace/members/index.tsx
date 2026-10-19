// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members section route (WSET-001/036, NEWFRONT-537). Thin: the settings
// layout owns the frame, the role gate and the tab title; this route
// hands the address to the shell's section outlet and loads the roster
// chunk, which registers its body in the page registry. The load starts
// from the route body (not a module import) so the roster stays out of
// the initial bundle.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { SettingsSectionOutlet } from "../../../../../../features/workspace-settings/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/members/")({
  component: WorkspaceSettingsSectionPage,
});

function WorkspaceSettingsSectionPage(): React.ReactElement {
  const { ws } = Route.useParams();
  React.useEffect(() => {
    void import("../../../../../../features/workspace-settings/members/register.js");
  }, []);
  return <SettingsSectionOutlet workspaceSlug={ws} section="members" />;
}
