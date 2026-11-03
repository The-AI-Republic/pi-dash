// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public marketing layout (CLOUD-003/004, parity/specs/cloud.md §1). One
// pathless route under the root wraps /home, /docs, /downloads and
// /pricing in the shared frame and owns their crawler metadata. It renders
// no providers and reads no session state, so the prerender build can
// mount the surface directly; the route children carry only their
// declarations, so the page body comes from the lazy surface and the
// marketing copy and icons stay out of the initial bundle every other
// area pays for. An edition that does not declare the address never
// matches a child, and the layout answers not-found.
import { createFileRoute, useLocation } from "@tanstack/react-router";
import * as React from "react";

import { edition, isEditionPublicRouteEnabled, publicMarketingPathOf } from "../core/edition/index.js";
import { NotFoundPage } from "../shared/shell/index.js";

const MarketingSurface = React.lazy(() => import("../features/marketing/components/MarketingSurface.js"));

export const Route = createFileRoute("/_marketing")({
  // Module-local (not exported): only a local component lets the router
  // split the layout out of the entry chunk, the same rule the login route
  // follows.
  component: function MarketingLayout() {
    const { pathname } = useLocation();
    // An edition that does not declare the address exposes no marketing
    // surface there: the route behaves as absent, exactly like any unknown
    // path (the OSS build keeps every one of these 404s).
    const page = publicMarketingPathOf(pathname);
    if (page === null || !isEditionPublicRouteEnabled(edition, page)) {
      return <NotFoundPage />;
    }
    return (
      <React.Suspense fallback={null}>
        <MarketingSurface path={page} />
      </React.Suspense>
    );
  },
});
