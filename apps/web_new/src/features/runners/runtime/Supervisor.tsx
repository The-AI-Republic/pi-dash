// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Background agent-runtime supervisor (RUN-044; PIDESKAPP-222/223). Desktop
// only: on web and cloud the platform exposes no runtime capability, so
// this renders nothing and runs no step. Mounted once in the signed-in
// workspace layout. On desktop it resumes the runtime for the signed
// account (discovery of this computer's Pi Dash daemon is evaluated for
// it), registers the local chat transport, re-checks the daemon and the
// engine sign-in on a minute cadence plus window focus and network recovery
// (the re-check also wakes the daemon's cloud connections — PIDESKAPP-181),
// moves older
// builds' private daemon trees to it (PIDESKAPP-223, design §13), asks
// once per machine whether it replaces an offline one (§4.10), and hosts
// the "Set up Pi Dash on this computer" consent dialog.
//
// Opening a project enrols nothing and shows no notice: the daemon hosts
// the engine whether or not the app is open, and a project gets its
// engine runner the first time a run uses it (design §5, PO-15).

import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { useMe } from "../../../core/session/index.js";
import { migrateLegacyTrees } from "./desktop-migration.js";
import { offerReplace } from "./desktop-replace.js";
import { compatibleHello, currentDiscovery, refreshRuntime, resumeRuntime, signInWorkspace } from "./service.js";

/** Background daemon re-check cadence (plus every window focus). */
export const RUNTIME_REFRESH_INTERVAL_MS = 60_000;

export interface RuntimeSupervisorProps {
  /** Registers the local chat transport once the runtime resumes (desktop chat slice). */
  registerChatTransport?: (() => void) | undefined;
  /** The consent dialog host; tests replace it. */
  setupDialog?: React.ComponentType | undefined;
  /** The migration-notices host; tests stub it. */
  noticesDialog?: ((props: { account: string }) => React.ReactElement) | undefined;
  /** The replace-machine host; tests stub it. */
  replaceDialog?: React.ComponentType | undefined;
}

/** Install the local chat transport once its chunk has loaded. */
function registerLocalChatTransportLazily(): void {
  void import("../chat/localBootstrap.js").then(
    (module) => module.registerLocalChatTransport(),
    () => undefined
  );
}

// The consent dialog and its kit controls load on demand, off the
// first-paint chain; most builds never take this path.
const LazyDaemonSetupDialog = React.lazy(() =>
  import("./DaemonSetupDialog.js").then((module) => ({ default: module.DaemonSetupDialog }))
);
const LazyMigrationNoticesDialog = React.lazy(() =>
  import("./MigrationNoticesDialog.js").then((module) => ({ default: module.MigrationNoticesDialog }))
);
const LazyReplaceMachineDialog = React.lazy(() =>
  import("./ReplaceMachineDialog.js").then((module) => ({ default: module.ReplaceMachineDialog }))
);

export function RuntimeSupervisor(props: RuntimeSupervisorProps): React.ReactElement | null {
  const registerChatTransport = props.registerChatTransport ?? registerLocalChatTransportLazily;
  const SetupDialog = props.setupDialog ?? LazyDaemonSetupDialog;
  const NoticesDialog = props.noticesDialog ?? LazyMigrationNoticesDialog;
  const ReplaceDialog = props.replaceDialog ?? LazyReplaceMachineDialog;
  const client = getAppClient();
  const runtime = platform.agentRuntime;
  const { data: me } = useMe();
  const userId = me?.id;
  const email = me?.email;
  // The cloud the app is signed in to, as the daemon's `Hello` names it.
  const cloudUrl = platform.apiOrigin ?? client.baseUrl;

  React.useEffect(() => {
    if (!runtime || !userId) return;
    resumeRuntime({ id: userId, email, cloudUrl }, { client, runtime });
    registerChatTransport?.();
    let active = true;
    let migrated = false;
    const askedFor = new Set<string>();
    const refresh = async (): Promise<void> => {
      await refreshRuntime({ client, runtime }).catch(() => undefined);
      if (!active) return;
      if (!migrated) {
        const cli = platform.cliSetup;
        const outcome = await migrateLegacyTrees(userId, {
          runtime,
          client,
          cli,
          cloudUrl,
          discovery: () => currentDiscovery(),
          signInWorkspace: (workspaceSlug: string) => signInWorkspace({ client, runtime, cli, workspaceSlug }),
        }).catch(() => ({ status: "nothing_to_do" }) as const);
        // Blocked (no daemon yet, another account): nothing happened, so try
        // again on the next refresh — set-up may finish meanwhile. A tree
        // that failed half-way waits for the next launch rather than
        // retrying every minute.
        if (outcome.status !== "blocked") migrated = true;
        if (outcome.status === "done" && outcome.failed.length) {
          console.warn("Moving to Pi Dash on this computer did not finish for", outcome.failed);
        }
      }
      // "Is this <label>? Replace it": once per launch for each machine the
      // daemon is signed in as (a fresh sign-in is a new machine id).
      const machineId = compatibleHello(currentDiscovery())?.machine_id;
      if (!active || !migrated || !machineId || askedFor.has(machineId)) return;
      askedFor.add(machineId);
      // A failed lookup (offline) is not an answer: ask again next refresh.
      await offerReplace(userId, {
        runtime,
        client,
        cli: platform.cliSetup,
        cloudUrl,
        discovery: () => currentDiscovery(),
        signInWorkspace: (workspaceSlug: string) =>
          signInWorkspace({ client, runtime, cli: platform.cliSetup, workspaceSlug }),
      }).catch(() => {
        askedFor.delete(machineId);
      });
    };
    const tick = (): void => {
      void refresh().catch(() => undefined);
    };
    tick();
    const timer = setInterval(tick, RUNTIME_REFRESH_INTERVAL_MS);
    const unsubscribe = platform.onFocusChange((focused) => {
      if (focused) tick();
    });
    // Network recovery: the refresh also wakes the daemon's cloud
    // connections — the browser's `online` event is the only prompt the
    // app gets when connectivity returns (PIDESKAPP-181).
    globalThis.window.addEventListener("online", tick);
    return () => {
      active = false;
      clearInterval(timer);
      unsubscribe();
      globalThis.window.removeEventListener("online", tick);
    };
  }, [runtime, userId, email, cloudUrl, client, registerChatTransport]);

  if (!runtime || !userId) return null;
  return (
    <React.Suspense fallback={null}>
      <SetupDialog />
      <NoticesDialog account={userId} />
      <ReplaceDialog />
    </React.Suspense>
  );
}
