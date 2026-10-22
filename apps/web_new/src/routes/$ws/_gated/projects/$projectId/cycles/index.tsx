// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project cycles tab home (parity/specs/cycles.md §1 and §5). The
// cycles area owns this screen (NEWFRONT-707): the list shell with its
// feature gate, zero-state, skeleton and error. Later slices fill the
// list body (NEWFRONT-708) and wire the create dialog slot (NEWFRONT-710).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

// Direct module import: the feature barrel grows with later slices
// (dialogs, pickers), which must not join this route's initial chunk.
import { CyclesListScreen } from "../../../../../../features/cycles/components/CyclesListScreen.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/cycles/")({
  component: CyclesComponent,
});

function CyclesComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return <CyclesListScreen workspaceSlug={ws} projectId={projectId} />;
}
