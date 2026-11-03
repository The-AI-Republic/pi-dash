// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project modules tab home (parity/specs/modules.md §2.1, §5, MOD-001,
// MOD-019). The Modules area owns this screen: the list route shell with
// its gate, layout-matched skeleton and persisted layout switcher, plus
// the `peekModule` search key the quick look opens through (slice D). The
// detail route keeps its own host.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import type { ModulesListScreen as ModulesListScreenComponent } from "../../../../../../features/modules/components/ModulesListScreen.js";
import { parseModulesListSearch } from "../../../../../../features/modules/lib/moduleListSearch.js";
import { withModulePeek } from "../../../../../../features/modules/lib/modulePeek.js";

// Deferred (size gate): the eager route definition only needs the loader;
// the modules list screen rides its own chunk.
const ModulesListScreen = React.lazy(() =>
  import("../../../../../../features/modules/components/ModulesListScreen.js").then((module) => ({
    default: module.ModulesListScreen,
  }))
) as typeof ModulesListScreenComponent;

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/modules/")({
  validateSearch: (search: Record<string, unknown>) => parseModulesListSearch(search),
  component: ModulesComponent,
});

function ModulesComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  // The peek lives in the address (§5): opening writes the key, closing
  // removes it, and both replace the entry so Back leaves the screen
  // rather than stepping through every panel the viewer opened.
  const openPeek = React.useCallback(
    (moduleId: string | null): void => {
      void navigate({ to: ".", search: withModulePeek(search, moduleId), replace: true } as never);
    },
    [navigate, search]
  );
  return (
    <React.Suspense fallback={null}>
      <ModulesListScreen
        workspaceSlug={ws}
        projectId={projectId}
        peekModuleId={search.peekModule ?? null}
        onPeekChange={openPeek}
      />
    </React.Suspense>
  );
}
