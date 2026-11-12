// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Registration entry for the upkeep tab (parity/specs/profile.md §5 seam,
// §11). Loading the page on demand keeps the shell's chunk free of the
// upkeep queries and their optimistic helpers.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const LazyUpkeepSettingsPage = React.lazy(() =>
  import("./UpkeepSettingsPage.js").then((module) => ({ default: module.UpkeepSettingsPage }))
);

export function UpkeepSettingsTabFallback(): React.ReactElement {
  return (
    <div data-testid="upkeep-settings-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
      <Skeleton className="h-6 w-48" />
      <Skeleton className="h-4 w-full" />
      <Skeleton className="h-4 w-2/3" />
    </div>
  );
}

export function UpkeepSettingsTab(): React.ReactElement {
  return (
    <React.Suspense fallback={<UpkeepSettingsTabFallback />}>
      <LazyUpkeepSettingsPage />
    </React.Suspense>
  );
}
