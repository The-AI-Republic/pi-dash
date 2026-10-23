// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public boards layout (parity/specs/space.md §1.1). This subtree is lazy,
// web-only and outside the workspace shell: no session gate, sidebar or
// workspace chrome. It owns the public runtime's query provider, the boot
// probe, the subtree error boundary and the toast/composition slots later
// space slices mount into.

import { QueryClientProvider } from "@tanstack/react-query";
import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { spaceRuntime } from "../../features/spaces/api/client.js";
import { SpaceCrashScreen } from "../../features/spaces/components/CrashScreen.js";
import { SpaceBootGate } from "../../features/spaces/components/SpaceBootGate.js";

export const Route = createFileRoute("/spaces")({
  errorComponent: SpaceCrashScreen,
  component: function SpacesLayout() {
    return (
      <QueryClientProvider client={spaceRuntime.queryClient}>
        <SpaceBootGate>
          <Outlet />
        </SpaceBootGate>
      </QueryClientProvider>
    );
  },
});
