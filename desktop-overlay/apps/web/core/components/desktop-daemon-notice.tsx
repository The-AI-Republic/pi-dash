/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState, useSyncExternalStore } from "react";
import {
  getDaemonNotice,
  repairSharedDaemon,
  subscribeDaemonNotice,
  updateCli,
} from "@/services/desktop-account-sync";

/**
 * The corner strip for the shared `pidash` daemon, shown only when something
 * needs the user:
 *
 * - the installed CLI is too old for this app → "Update the Pi Dash CLI";
 * - the OS service could not be installed, so the app runs the daemon itself
 *   → "Runs only while the app is open", with Repair to retry the service;
 * - no daemon could be started at all → why, with Repair.
 */
export function DesktopDaemonNotice() {
  const notice = useSyncExternalStore(subscribeDaemonNotice, getDaemonNotice);
  const [error, setError] = useState("");

  const act = (action: () => Promise<unknown>) => {
    setError("");
    void action().catch((reason: unknown) => {
      setError(String(reason instanceof Error ? reason.message : reason));
    });
  };

  let text: string;
  let label: string;
  let action: () => Promise<unknown>;
  if (notice.cliOutdated) {
    text = "The pidash CLI on this computer is out of date.";
    label = "Update the Pi Dash CLI";
    action = updateCli;
  } else if (notice.daemon?.mode === "child") {
    text = "Runs only while the app is open.";
    label = "Repair";
    action = repairSharedDaemon;
  } else if (notice.daemon?.mode === "unavailable") {
    text = notice.daemon.message ?? "The Pi Dash daemon is not running.";
    label = "Repair";
    action = repairSharedDaemon;
  } else {
    return null;
  }

  return (
    <div
      role="status"
      aria-label="Pi Dash daemon"
      className="shadow-sm fixed right-2 bottom-2 z-20 flex max-w-sm items-center gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-body-xs-regular"
    >
      <span>{error || text}</span>
      <button
        type="button"
        className="shrink-0 rounded-sm border border-subtle px-2 py-0.5 text-body-xs-medium disabled:opacity-60"
        disabled={notice.busy}
        onClick={() => act(action)}
      >
        {notice.busy ? "Working…" : label}
      </button>
    </div>
  );
}
