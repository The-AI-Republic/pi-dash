// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Router harness for the project-settings suites. A memory router at the
// tested address provides Link and router-state context; the root route
// renders the component under test.

import { createMemoryHistory, createRootRoute, createRouter, RouterProvider } from "@tanstack/react-router";
import * as React from "react";

export function renderInRouter(pathname: string, node: React.ReactElement): JSX.Element {
  const rootRoute = createRootRoute({ component: () => node });
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: [pathname] }),
    trailingSlash: "preserve",
  });
  return <RouterProvider router={router} />;
}
