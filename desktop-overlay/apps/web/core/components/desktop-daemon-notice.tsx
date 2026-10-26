/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState, useSyncExternalStore } from "react";
import type { UnadoptedRunner } from "@/services/desktop-account-sync";
import {
  getDaemonNotice,
  repairSharedDaemon,
  subscribeDaemonNotice,
  updateCli,
} from "@/services/desktop-account-sync";

const STRIP =
  "shadow-sm flex max-w-sm items-center gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-body-xs-regular";
const BUTTON =
  "shrink-0 rounded-sm border border-subtle px-2 py-0.5 text-body-xs-medium disabled:opacity-60";

/** "a", "a and b", "a, b and c". */
function listNames(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** Why adopt left a runner behind, as far as the notice needs to say. */
function skipKind(reason: string): "workspace" | "cloud" | "config" {
  if (reason === "cloud_mismatch") return "cloud";
  if (reason === "invalid_config") return "config";
  // workspace_mismatch, not_signed_in, workspace_unknown — and anything newer.
  return "workspace";
}

/**
 * One sentence per workspace (and kind of reason) for runners an earlier
 * version of the app ran and the shared CLI could not take over. Their old
 * config, sign-in included, is kept; the ones from another workspace move
 * over by themselves once the CLI is signed in to it.
 */
export function unadoptedMessages(runners: UnadoptedRunner[]): string[] {
  const groups = new Map<string, { workspace: string; kind: ReturnType<typeof skipKind>; names: string[] }>();
  for (const runner of runners) {
    const kind = skipKind(runner.reason);
    const key = `${kind}\u0000${runner.workspace}`;
    const group = groups.get(key) ?? { workspace: runner.workspace, kind, names: [] };
    group.names.push(runner.name);
    groups.set(key, group);
  }
  return [...groups.values()].map(({ workspace, kind, names }) => {
    const one = names.length === 1;
    const subject = one ? `Runner ${names[0]}` : `Runners ${listNames(names)}`;
    const where = workspace ? `workspace ${workspace}` : "another workspace";
    if (kind === "cloud") {
      return `${subject} from an earlier version of this app ${one ? "is" : "are"} registered with a different Pi Dash server and ${one ? "was" : "were"} not moved to the pidash CLI on this computer.`;
    }
    if (kind === "config") {
      return `${subject} from an earlier version of this app could not be moved to the pidash CLI on this computer: ${one ? "its" : "their"} settings clash with a runner it already has.`;
    }
    return (
      `${subject} ${one ? "belongs" : "belong"} to ${where} and can't run until the pidash CLI on this computer is signed in to that workspace. ` +
      "Running several workspaces at once is planned for a later update."
    );
  });
}

/**
 * The corner strip for the shared `pidash` daemon, shown only when something
 * needs the user:
 *
 * - the installed CLI is too old for this app → "Update the Pi Dash CLI";
 * - the OS service could not be installed, so the app runs the daemon itself
 *   → "Runs only while the app is open", with Repair to retry the service;
 * - no daemon could be started at all → why, with Repair;
 * - runners from an earlier version of the app could not move to the shared
 *   CLI because it is signed in to another workspace (or not at all) → which
 *   ones and why, until dismissed.
 */
export function DesktopDaemonNotice() {
  const notice = useSyncExternalStore(subscribeDaemonNotice, getDaemonNotice);
  const [error, setError] = useState("");
  const [dismissed, setDismissed] = useState("");

  const act = (action: () => Promise<unknown>) => {
    setError("");
    void action().catch((reason: unknown) => {
      setError(String(reason instanceof Error ? reason.message : reason));
    });
  };

  let text: string | null = null;
  let label = "";
  let action: () => Promise<unknown> = async () => {};
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
  }

  const unadopted = unadoptedMessages(notice.daemon?.unadopted ?? []);
  const unadoptedKey = unadopted.join("\n");
  const showUnadopted = unadopted.length > 0 && dismissed !== unadoptedKey;

  if (text === null && !showUnadopted) return null;

  return (
    <div className="fixed right-2 bottom-2 z-20 flex flex-col items-end gap-2">
      {showUnadopted && (
        <div role="status" aria-label="Runners not moved" className={STRIP}>
          <span className="flex flex-col gap-1">
            {unadopted.map((message) => (
              <span key={message}>{message}</span>
            ))}
          </span>
          <button type="button" className={BUTTON} onClick={() => setDismissed(unadoptedKey)}>
            Dismiss
          </button>
        </div>
      )}
      {text !== null && (
        <div role="status" aria-label="Pi Dash daemon" className={STRIP}>
          <span>{error || text}</span>
          <button type="button" className={BUTTON} disabled={notice.busy} onClick={() => act(action)}>
            {notice.busy ? "Working…" : label}
          </button>
        </div>
      )}
    </div>
  );
}
