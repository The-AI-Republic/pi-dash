// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project modules tab home (parity/specs/modules.md §2.1, MOD-001). The
// Modules area owns this screen: the list route shell with its gate,
// layout-matched skeleton and persisted layout switcher. The detail route
// keeps its own host.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import type { ModulesListScreen as ModulesListScreenComponent } from "../../../../../../features/modules/components/ModulesListScreen.js";

// Deferred (size gate): the eager route definition only needs the loader;
// the modules list screen rides its own chunk.
const ModulesListScreen = React.lazy(() =>
  import("../../../../../../features/modules/components/ModulesListScreen.js").then((module) => ({
    default: module.ModulesListScreen,
  }))
) as typeof ModulesListScreenComponent;

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/modules/")({
  component: ModulesComponent,
});

function ModulesComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <React.Suspense fallback={null}>
      <ModulesListScreen workspaceSlug={ws} projectId={projectId} />
    </React.Suspense>
  );
}
