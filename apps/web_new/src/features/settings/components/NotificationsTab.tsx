// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notifications tab body (parity/specs/profile.md §8/§13, PROF-018;
// NTF-024/025). The notifications area owns the toggles and their saves;
// this module only mounts its page inside the settings seam, loading it
// on demand so the settings shell never carries the preference code.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const LazyEmailPreferencesPage = React.lazy(() =>
  import("../../notifications/index.js").then((module) => ({ default: module.EmailPreferencesPage }))
);

export function NotificationsTab(): React.ReactElement {
  return (
    <React.Suspense
      fallback={
        <div aria-busy="true" className="flex flex-col gap-(--space-3)">
          <Skeleton className="h-6 w-48" />
          <Skeleton className="h-4 w-full" />
        </div>
      }
    >
      <LazyEmailPreferencesPage />
    </React.Suspense>
  );
}
