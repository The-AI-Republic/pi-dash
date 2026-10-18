// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Root route. Owns the app providers (Query + router context) so every
// screen reads data through loaders and feature hooks, the document shell
// defaults (product title, crawler directives, initial theme mark), and
// the illustrated not-found surface. Area epics add their subtrees under
// $ws/, god-mode/ and spaces/.

import type { ApiClient } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { LanguageProvider } from "../core/i18n/LanguageProvider.js";
import { ThemeProvider } from "../core/theme/ThemeProvider.js";
import { RuntimeSignOutLifecycle } from "../features/runners/runtime/SignOutLifecycle.js";
import { NotFoundPage, markInitialColorScheme, notifyError, useDocumentHead } from "../shared/shell/index.js";

export interface RouterContext {
  queryClient: QueryClient;
  apiClient: ApiClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  notFoundComponent: NotFoundPage,
  component: function RootComponent() {
    const context = Route.useRouteContext();
    useDocumentHead({ title: "Pi Dash", robots: "index, nofollow", fallback: true });
    React.useEffect(() => {
      markInitialColorScheme();
    }, []);
    return (
      <QueryClientProvider client={context.queryClient}>
        <ThemeProvider notifyError={notifyError}>
          <LanguageProvider notifyError={notifyError}>
            <RuntimeSignOutLifecycle />
            <Outlet />
          </LanguageProvider>
        </ThemeProvider>
      </QueryClientProvider>
    );
  },
});
