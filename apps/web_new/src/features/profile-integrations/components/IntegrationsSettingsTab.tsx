// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Registration entry for the integrations tab (parity/specs/profile.md §5
// seam, §11). The lazy split keeps the status read and the panel out of
// the shell chunk.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const LazyIntegrationsSettingsPage = React.lazy(() =>
  import("./IntegrationsSettingsPage.js").then((module) => ({ default: module.IntegrationsSettingsPage }))
);

export function IntegrationsSettingsTabFallback(): React.ReactElement {
  return (
    <div data-testid="integrations-settings-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
      <Skeleton className="h-6 w-48" />
      <Skeleton className="h-4 w-full" />
      <Skeleton className="h-4 w-2/3" />
    </div>
  );
}

export function IntegrationsSettingsTab(): React.ReactElement {
  return (
    <React.Suspense fallback={<IntegrationsSettingsTabFallback />}>
      <LazyIntegrationsSettingsPage />
    </React.Suspense>
  );
}
