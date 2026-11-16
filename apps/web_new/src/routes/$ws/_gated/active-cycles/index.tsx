// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace active cycles (parity/specs/cycles.md §16, CYC-043).
// Thin: mounts the paid-feature upsell. The key remounts per
// workspace so no upsell state leaks across workspaces.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

// Deep import (size gate): the cycles barrel also re-exports the transfer
// hook, which depends on the issues barrel. This route only mounts the
// upsell, so keep that larger closure off the eager route tree.
import { WorkspaceActivesUpsell } from "../../../../features/cycles/components/WorkspaceActivesUpsell.js";

export const Route = createFileRoute("/$ws/_gated/active-cycles/")({
  component: ActivesComponent,
});

function ActivesComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  return <WorkspaceActivesUpsell key={ws} />;
}
