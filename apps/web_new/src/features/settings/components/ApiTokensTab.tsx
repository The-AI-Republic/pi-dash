// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal access tokens tab body (parity/specs/profile.md §9, PROF-020;
// workspace-settings spec §41 seam). The token screens are built once by
// the workspace-settings area; this module only adopts that page into the
// settings seam, loading it on demand so the settings shell never carries
// the token dialogs or their data hooks in its own chunk.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const LazyPersonalAccessTokensPage = React.lazy(() =>
  import("../../workspace-settings/tokens/index.js").then((module) => ({
    default: module.PersonalAccessTokensPage,
  }))
);

export function ApiTokensTab(): React.ReactElement {
  return (
    <React.Suspense
      fallback={
        <div data-testid="api-tokens-tab-fallback" aria-busy="true" className="flex flex-col gap-(--space-3)">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      }
    >
      <LazyPersonalAccessTokensPage />
    </React.Suspense>
  );
}
