// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list route (parity/specs/views.md §9, VIEW-029-032/039).
// The gated layout already holds the session; this route only hands the
// workspace slug to the views-owned screen. Route modules are part of the
// eager route graph, so the screen and its read stay behind the router's
// component split.
//
// The address sits beside `all-issues`, whose own route mounts the global
// issue list: `/workspace-views` (no trailing segment) is the list, and a
// key or id after it is a view. The detail routes land with their slices.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { WorkspaceViewsListScreen } from "../../../../features/views/index.js";

export const Route = createFileRoute("/$ws/_gated/workspace-views/")({
  component: WorkspaceViewsComponent,
});

function WorkspaceViewsComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <WorkspaceViewsListScreen workspaceSlug={ws} />;
}
