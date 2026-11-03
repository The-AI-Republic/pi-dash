// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page detail address (parity/specs/pages.md §2.6, §4, PAGE-022–025,
// 079–080). The route owns only the two shareable query keys; the screen
// reads them and writes them back through the router so a pane tab or a
// snapshot is part of the address itself.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { PageDetailScreen } from "../../../../../../features/pages/components/PageDetailScreen.js";
import { loadPageExportRegistration } from "../../../../../../features/pages-export/lib/loadPageExportRegistration.js";
import { parsePageDetailSearch, type PageDetailSearch } from "../../../../../../features/pages/lib/pageDetail.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/pages/$pageId")({
  validateSearch: (search: Record<string, unknown>) => parsePageDetailSearch(search),
  component: PageDetailComponent,
});

function PageDetailComponent(): React.ReactElement {
  const { ws, projectId, pageId } = Route.useParams();
  // Narrow again: the router merges each parent match's unvalidated search
  // under the child's validated keys, so an unknown ?paneTab=zzz arrives
  // raw here despite validateSearch — and the pane's presence check would
  // open on it (PAGE-045). Re-parsing keeps the one choke point narrow.
  const search = parsePageDetailSearch({ ...Route.useSearch() });
  const navigate = useNavigate();
  // The export chunk registers its dialog into the detail seam on import,
  // so the load starts from the route body and the dialog stays out of the
  // initial bundle (PAGE-074, NEWFRONT-846). The helper consumes a failed
  // chunk load; otherwise closing a worker mid-fetch would leave an
  // unhandled rejection in the suite.
  React.useEffect(() => {
    void loadPageExportRegistration(() => import("../../../../../../features/pages-export/register.js"));
  }, []);
  return (
    <PageDetailScreen
      workspaceSlug={ws}
      projectId={projectId}
      pageId={pageId}
      search={search as PageDetailSearch}
      onSearchChange={(next) => {
        void navigate({
          to: "/$ws/projects/$projectId/pages/$pageId",
          params: { ws, projectId, pageId },
          search: next,
          replace: true,
        });
      }}
    />
  );
}
