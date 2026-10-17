// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace Apps catalog route (CLOUD-021/022, written from
// parity/specs/cloud.md §5). Thin: the trailing-slash landing and the
// edition gate. The cloud edition declares this route in
// `edition.routes` (see core/edition/routes.ts for the module
// contract); every other build renders the not-found page, so the
// route 404s exactly like an undeclared address. Reads fetch on mount
// (CLOUD-030): a loader prefetch would cache the first failure and its
// mount refetch would consume the oracle's fail-once-then-live second
// call before the error panel ever renders — and the marketplace
// window must only load behind a connected status, which no loader
// can know before the status read lands.
import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { APPS_CATALOG_ROUTE_PATH, edition, isEditionRouteEnabled } from "../../../../core/edition/index.js";
import { AppsView } from "../../../../features/openhub/index.js";
import { NotFoundPage } from "../../../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/_gated/apps/")({
  beforeLoad: ({ location }) => {
    // Django-style trailing slash (CLOUD-021): /<slug>/apps lands on
    // /<slug>/apps/, carrying any query string and fragment along.
    if (!location.pathname.endsWith("/")) {
      const hash = location.hash === "" ? "" : `#${location.hash}`;
      throw redirect({ href: `${location.pathname}/${location.searchStr}${hash}` });
    }
  },
  component: AppsRoute,
});

function AppsRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  if (!isEditionRouteEnabled(edition, APPS_CATALOG_ROUTE_PATH)) {
    return <NotFoundPage />;
  }
  return <AppsView key={ws} workspaceSlug={ws} />;
}
