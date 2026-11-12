// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace Apps catalog route (CLOUD-021/022, written from
// parity/specs/cloud.md §5). Thin: the edition gate. The cloud
// edition declares this route in `edition.routes` (see
// core/edition/routes.ts for the module contract); every other build
// renders the not-found page, so the route 404s exactly like an
// undeclared address. Both /<slug>/apps and /<slug>/apps/ serve (the
// oracle answers the slashless address with 200 and no redirect, so
// there is deliberately no trailing-slash landing here — NEWFRONT-662:
// the generated route path is slashless, so a beforeLoad redirect to
// the slashed self re-triggers beforeLoad forever and OOMs the tab).
// Reads fetch on mount (CLOUD-030): a loader prefetch would cache the
// first failure and its mount refetch would consume the oracle's
// fail-once-then-live second call before the error panel ever renders —
// and the marketplace window must only load behind a connected status,
// which no loader can know before the status read lands.
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { APPS_CATALOG_ROUTE_PATH, edition, isEditionRouteEnabled } from "../../../../core/edition/index.js";
import { AppsView } from "../../../../features/openhub/components/AppsView.js";
import { NotFoundPage } from "../../../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/_gated/apps/")({
  component: AppsRoute,
});

function AppsRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  if (!isEditionRouteEnabled(edition, APPS_CATALOG_ROUTE_PATH)) {
    return <NotFoundPage />;
  }
  return <AppsView key={ws} workspaceSlug={ws} />;
}
