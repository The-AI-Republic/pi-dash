// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Root startup gate (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). Every workspace route — member pages, god-mode and auth —
// renders beneath this one decision, so maintenance and unfinished setup
// replace the whole app and never leak workspace chrome. Two subtrees are
// exceptions: the administrator entry must stay reachable while setup is
// unfinished or the read fails because it owns the recovery forms, and the
// public boards subtree owns its own single-attempt probe and failure screen
// (SPC-018) without workspace chrome. The loader is a full-screen spinner
// until the single read settles; the gate then hands control to the router
// and does not remount on navigation.

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

/** The public boards subtree owns its own boot probe and failure surface. */
export function isSpacesPath(pathname: string): boolean {
  return pathname === "/spaces" || pathname.startsWith("/spaces/");
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
 * The startup decision for every workspace route: one instance read then a
 * branch on it.
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
  const pathname = globalThis.window.location.pathname;
  // Both bypassed subtrees hand control back before the workspace probe is
  // mounted. The administrator subtree owns the only instance read on those
  // routes (ADM-001/ADM-003), and the public boards subtree owns its own
  // single-attempt probe and failure screen (SPC-018), so a visitor never
  // issues a second read and never loads workspace chrome.
  if (isAdminEntryPath(pathname) || isSpacesPath(pathname)) return <>{children}</>;
  return <StartupGate>{children}</StartupGate>;
}
