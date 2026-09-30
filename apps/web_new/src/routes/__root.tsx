// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Root route. Owns the app providers (Query + router context) so every
// screen reads data through loaders and feature hooks. Area epics add
// their subtrees under $ws/, god-mode/ and spaces/.

import type { ApiClient } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";

export interface RouterContext {
  queryClient: QueryClient;
  apiClient: ApiClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  notFoundComponent: function NotYetMigrated() {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center gap-(--space-2) bg-(--bg) p-(--space-8)">
        <h1 className="text-h2 text-(--text)">Not here yet</h1>
        <p className="text-body text-(--text-muted)">
          This area still lives in the old app and moves over in a later epic.
        </p>
      </main>
    );
  },
  component: function RootComponent() {
    const context = Route.useRouteContext();
    return (
      <QueryClientProvider client={context.queryClient}>
        <Outlet />
      </QueryClientProvider>
    );
  },
});
