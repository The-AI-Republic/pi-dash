// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Root startup gate (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). Every route — member pages, god-mode, spaces and auth —
// renders beneath this one decision, so maintenance and unfinished setup
// replace the whole app and never leak workspace chrome. The administrator
// entry subtree is the one exception: it must stay reachable while setup is
// unfinished or the read fails, because it owns the recovery forms. The
// loader is a full-screen spinner until the single read settles; the gate
// then hands control to the router and does not remount on navigation.

import { Spinner } from "@pidash/kit";
import * as React from "react";

import { useAppInstanceBootstrap } from "./queries.js";
import { decideAppInstanceGate } from "./status.js";
import { markInstanceBootScheme } from "./theme.js";

// The settled screens are the largest part of this feature but render only
// after the read reaches maintenance or unfinished setup. Loading them on
// demand keeps their copy and artwork out of the initial chain (size gate);
// the gate itself stays eager so the decision never waits on a chunk.
const LazyMaintenanceScreen = React.lazy(async () => {
  const module = await import("./MaintenanceScreen.js");
  return { default: module.MaintenanceScreen };
});

const LazyNotReadyScreen = React.lazy(async () => {
  const module = await import("./NotReadyScreen.js");
  return { default: module.NotReadyScreen };
});

/** The administrator entry subtree, including setup and sign-in. */
export function isAdminEntryPath(pathname: string): boolean {
  return pathname === "/god-mode" || pathname.startsWith("/god-mode/");
}

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

/**
 * The startup decision for every non-administrator route: one instance
 * read then a branch on it.
 */
function StartupGate({ children }: InstanceAppGateProps): React.ReactElement {
  const bootstrap = useAppInstanceBootstrap();
  const branch = decideAppInstanceGate({
    isPending: bootstrap.isPending,
    isError: bootstrap.isError,
    data: bootstrap.data,
  });
  if (branch === "loading") return <BootLoader />;
  if (branch === "maintenance") {
    return (
      <React.Suspense fallback={<BootLoader />}>
        <LazyMaintenanceScreen />
      </React.Suspense>
    );
  }
  if (branch === "not-ready") {
    return (
      <React.Suspense fallback={<BootLoader />}>
        <LazyNotReadyScreen />
      </React.Suspense>
    );
  }
  return <>{children}</>;
}

export function InstanceAppGate({ children }: InstanceAppGateProps): React.ReactElement {
  React.useLayoutEffect(() => {
    markInstanceBootScheme();
  }, []);
  // The administrator subtree renders before the startup read is even
  // asked for: its own gate owns the only instance read on those
  // routes (ADM-001/ADM-003), and the startup reader's background
  // retry would otherwise refetch behind that screen's back. Deciding
  // here also keeps the startup read from firing on a failed entry.
  if (isAdminEntryPath(globalThis.window.location.pathname)) return <>{children}</>;
  return <StartupGate>{children}</StartupGate>;
}
