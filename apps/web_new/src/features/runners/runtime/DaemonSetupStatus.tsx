// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Button } from "@pidash/kit";
import * as React from "react";
import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { agentFailureMessage } from "../../../shared/lib/agentReasons.js";
import { setUpDaemon, type DaemonSetUpStep } from "./service.js";
import { getSetupStatus, subscribeSetupStatus } from "./setupStatus.js";

export const SETUP_STEP_TEXT: Record<DaemonSetUpStep, string> = {
  install_cli: "Setting up Pi Dash: installing the command-line tool…",
  sign_in: "Setting up Pi Dash: signing in…",
  service: "Setting up Pi Dash: starting the background service…",
  engine: "Setting up Pi Dash: installing the Pi Dash Agent…",
  done: "Pi Dash is ready.",
};

export function DaemonSetupStatus({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement | null {
  const status = React.useSyncExternalStore(subscribeSetupStatus, getSetupStatus, () => null);
  if (!status) return null;
  return (
    <div className="shadow-lg fixed right-4 bottom-4 z-50 rounded-md bg-(--surface) p-4" aria-live="polite">
      {status.kind === "running" ? (
        <p role="status">{SETUP_STEP_TEXT[status.step]}</p>
      ) : (
        <>
          <p role="alert">Pi Dash setup failed: {agentFailureMessage(status.error)}</p>
          <Button
            onClick={() =>
              void setUpDaemon({
                client: getAppClient(),
                runtime: platform.agentRuntime,
                cli: platform.cliSetup,
                workspaceSlug,
              }).catch(() => undefined)
            }
          >
            Try again
          </Button>
        </>
      )}
    </div>
  );
}
