// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace active cycles (parity/specs/cycles.md §16, CYC-043).
// Thin: mounts the paid-feature upsell. The key remounts per
// workspace so no upsell state leaks across workspaces.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { WorkspaceActivesUpsell } from "../../../../features/cycles/index.js";

export const Route = createFileRoute("/$ws/_gated/active-cycles/")({
  component: ActivesComponent,
});

function ActivesComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <WorkspaceActivesUpsell key={ws} />;
}
