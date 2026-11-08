// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public boards layout (parity/specs/space.md §1.1). This subtree is lazy,
// web-only and outside the workspace shell: no session gate, sidebar or
// workspace chrome. The desktop bundle never mounts it (desktop spec §11,
// decision D13): the gate throws not-found before any board route loads,
// and the desktop not-found surface hands the address to the browser. It owns the public runtime's query provider, the boot
// probe, the subtree error boundary, the boot locale stamp, the toast
// host behind the space notices and the route progress slices report to.

import { ToastHost } from "@pidash/kit";
import { QueryClientProvider } from "@tanstack/react-query";
import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { platform } from "../../core/platform/index.js";
import { spaceRuntime } from "../../features/spaces/api/client.js";
import { SpaceCrashScreen } from "../../features/spaces/components/CrashScreen.js";
import { SpaceBootGate } from "../../features/spaces/components/SpaceBootGate.js";
import { SPACE_HEAD_ROBOTS, SPACE_HEAD_TITLE_FALLBACK } from "../../features/spaces/lib/boardHead.js";
import { useDocumentHead } from "../../shared/shell/document.js";
import { SpaceLocaleGate } from "../../features/spaces/components/SpaceLocaleGate.js";
import { SpaceProgressProvider, SpaceRouteProgress } from "../../features/spaces/components/SpaceRouteProgress.js";
import { getToastManager } from "../../shared/shell/toaster.js";

export const Route = createFileRoute("/spaces")({
  beforeLoad: () => {
    if (platform.kind === "desktop") throw notFound();
    return {};
  },
  errorComponent: SpaceCrashScreen,
  component: function SpacesLayout() {
    // Subtree crawler directive (SPC-008). A fallback claim: it beats the
    // app root's indexable default while every space leaf's own claim —
    // board tags, landing, failures — wins for its page.
    useDocumentHead({ title: SPACE_HEAD_TITLE_FALLBACK, robots: SPACE_HEAD_ROBOTS, fallback: true });
    return (
      <QueryClientProvider client={spaceRuntime.queryClient}>
        <SpaceLocaleGate />
        <SpaceProgressProvider>
          <SpaceRouteProgress />
          <ToastHost manager={getToastManager()} />
          <SpaceBootGate>
            <Outlet />
          </SpaceBootGate>
        </SpaceProgressProvider>
      </QueryClientProvider>
    );
  },
});
