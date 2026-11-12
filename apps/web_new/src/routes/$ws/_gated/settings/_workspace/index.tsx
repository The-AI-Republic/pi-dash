// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General workspace settings (parity/specs/workspace-settings.md §3–§6,
// WSET-003–006). Thin: the shell owns the frame and the gate; the general
// feature renders the page. Navigation is client-side so the delete flow
// lands through the router, not a reload.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { WorkspaceGeneralPage } from "../../../../../features/workspace-settings/general/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/_workspace/")({
  component: WorkspaceGeneralRoute,
});

function WorkspaceGeneralRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  const navigate = Route.useNavigate();
  return (
    <WorkspaceGeneralPage
      workspaceSlug={ws}
      onNavigate={(path) => {
        void navigate({ href: path });
      }}
    />
  );
}
