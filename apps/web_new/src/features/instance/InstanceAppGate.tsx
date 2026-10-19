// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Root startup gate (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). Every route — member pages, god-mode, spaces and auth —
// renders beneath this one decision, so maintenance and unfinished setup
// replace the whole app and never leak workspace chrome. The loader is a
// full-screen spinner until the single read settles; the gate then hands
// control to the router and does not remount on navigation.

import { Spinner } from "@pidash/kit";
import * as React from "react";

import { MaintenanceScreen } from "./MaintenanceScreen.js";
import { NotReadyScreen } from "./NotReadyScreen.js";
import { useAppInstanceBootstrap } from "./queries.js";
import { decideAppInstanceGate } from "./status.js";
import { markInstanceBootScheme } from "./theme.js";

export interface InstanceAppGateProps {
  children: React.ReactNode;
}

function BootLoader(): React.ReactElement {
  return (
    <div
      data-testid="instance-boot-loader"
      role="status"
      aria-label="Loading instance"
      className="flex min-h-screen items-center justify-center bg-(--bg)"
    >
      <Spinner size="large" label="Loading instance" />
    </div>
  );
}

export function InstanceAppGate({ children }: InstanceAppGateProps): React.ReactElement {
  React.useLayoutEffect(() => {
    markInstanceBootScheme();
  }, []);
  const bootstrap = useAppInstanceBootstrap();
  const branch = decideAppInstanceGate({
    isPending: bootstrap.isPending,
    isError: bootstrap.isError,
    data: bootstrap.data,
  });
  if (branch === "loading") return <BootLoader />;
  if (branch === "maintenance") return <MaintenanceScreen />;
  if (branch === "not-ready") return <NotReadyScreen />;
  return <>{children}</>;
}
