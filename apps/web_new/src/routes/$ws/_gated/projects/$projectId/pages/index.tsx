// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project pages tab home (parity/specs/pages.md §2.6 and §3.1). The one
// address-owned pages state is the active visibility tab (`type`); every
// other discovery dimension is client state inside the screen.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import type { PagesListScreen as PagesListScreenComponent } from "../../../../../../features/pages/components/PagesListScreen.js";
import { parsePageListSearch } from "../../../../../../features/pages/lib/pageListSearch.js";

// Deferred (size gate): the eager route definition owns the search
// validation; the list screen and its row actions ride their own chunk.
const PagesListScreen = React.lazy(() =>
  import("../../../../../../features/pages/components/PagesListScreen.js").then((module) => ({
    default: module.PagesListScreen,
  }))
) as typeof PagesListScreenComponent;

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/pages/")({
  validateSearch: (search: Record<string, unknown>) => parsePageListSearch(search),
  component: PagesComponent,
});

function PagesComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const { type } = Route.useSearch();
  return (
    <React.Suspense fallback={null}>
      <PagesListScreen workspaceSlug={ws} projectId={projectId} tab={type} />
    </React.Suspense>
  );
}
