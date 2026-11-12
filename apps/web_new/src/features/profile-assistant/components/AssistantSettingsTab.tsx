// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Registration entry for the AI-assistant tab (parity/specs/profile.md §5
// seam, §10). The settings shell imports this wrapper, which pulls the
// tab's code only when the member opens it, so the shell's own chunk never
// carries the assistant forms.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const LazyAssistantSettingsPage = React.lazy(() =>
  import("./AssistantSettingsPage.js").then((module) => ({ default: module.AssistantSettingsPage }))
);

export function AssistantSettingsTabFallback(): React.ReactElement {
  return (
    <div data-testid="assistant-settings-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
      <Skeleton className="h-6 w-48" />
      <Skeleton className="h-4 w-full" />
      <Skeleton className="h-4 w-2/3" />
    </div>
  );
}

export function AssistantSettingsTab(): React.ReactElement {
  return (
    <React.Suspense fallback={<AssistantSettingsTabFallback />}>
      <LazyAssistantSettingsPage />
    </React.Suspense>
  );
}
