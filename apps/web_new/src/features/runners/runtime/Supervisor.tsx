// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Background agent-runtime supervisor (RUN-044; PIDESKAPP-222). Desktop
// only: on web and cloud the platform exposes no runtime capability, so
// this renders nothing and runs no step. Mounted once in the signed-in
// workspace layout. On desktop it resumes the runtime for the signed
// account (discovery of this computer's Pi Dash daemon is evaluated for
// it), registers the local chat transport, re-checks the daemon and the
// engine sign-in on a minute cadence plus window focus, and hosts the
// "Set up Pi Dash on this computer" consent dialog.
//
// Opening a project enrols nothing and shows no notice: the daemon hosts
// the engine whether or not the app is open, and a project gets its
// engine runner the first time a run uses it (design §5, PO-15).

import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { useMe } from "../../../core/session/index.js";
import { refreshRuntime, resumeRuntime } from "./service.js";

/** Background daemon re-check cadence (plus every window focus). */
export const RUNTIME_REFRESH_INTERVAL_MS = 60_000;

export interface RuntimeSupervisorProps {
  /** Registers the local chat transport once the runtime resumes (desktop chat slice). */
  registerChatTransport?: (() => void) | undefined;
  /** The consent dialog host; tests replace it. */
  setupDialog?: React.ComponentType | undefined;
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

export function RuntimeSupervisor(props: RuntimeSupervisorProps): React.ReactElement | null {
  const registerChatTransport = props.registerChatTransport ?? registerLocalChatTransportLazily;
  const SetupDialog = props.setupDialog ?? LazyDaemonSetupDialog;
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
    const refresh = (): void => {
      void refreshRuntime({ client, runtime }).catch(() => undefined);
    };
    refresh();
    const timer = setInterval(refresh, RUNTIME_REFRESH_INTERVAL_MS);
    const unsubscribe = platform.onFocusChange((focused) => {
      if (focused) refresh();
    });
    return () => {
      clearInterval(timer);
      unsubscribe();
    };
  }, [runtime, userId, email, cloudUrl, client, registerChatTransport]);

  if (!runtime || !userId) return null;
  return (
    <React.Suspense fallback={null}>
      <SetupDialog />
    </React.Suspense>
  );
}
