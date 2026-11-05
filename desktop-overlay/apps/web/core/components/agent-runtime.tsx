/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import { useUser } from "@/hooks/store/user";
import { DesktopDaemonSetupDialog } from "@/components/desktop-daemon-setup-dialog";
import { DesktopMigrationNotices } from "@/components/desktop-migration-notices";
import { DesktopReplaceMachineDialog } from "@/components/desktop-replace-machine-dialog";
import { DesktopSignOutDialog } from "@/components/desktop-sign-out-dialog";
import { isDesktop, refreshAgentRuntime, resumeAgentRuntime } from "@/services/agent-runtime";
import { compatibleHello } from "@/services/desktop-daemon";
import { migrateLegacyTrees } from "@/services/desktop-migration";
import { offerReplace } from "@/services/desktop-replace";
import { registerLocalChatTransport } from "@/services/local-chat-transport";

/**
 * Desktop runtime, mounted on every page (`app/provider.tsx`).
 *
 * Opening a project no longer enrols anything: the built-in engine is turned
 * on for a project when it is used (design §5). What runs here, per signed-in
 * user: install the per-runner chat routing, find this computer's daemon (on
 * launch, on focus and every minute — renewing the engine silently only when
 * its sign-in is `expiring` or `expired`), and move older builds' private
 * daemon trees to it. The migration is retried on each launch until the
 * daemon is there and signed in as this account.
 *
 * Once the migration is out of the way, it asks — once per launch and machine
 * — whether the daemon's machine replaces an offline one with the same
 * hostname (design §4.10), and it shows the migration's one-time notices.
 */
export const AgentRuntime = observer(function AgentRuntime() {
  const { data: user } = useUser();

  useEffect(() => {
    if (!isDesktop() || !user?.id) return;
    resumeAgentRuntime(user.id, user.email);
    // Local for "This computer", the cloud for every other runner.
    registerLocalChatTransport();
    let active = true;
    let migrated = false;
    const askedFor = new Set<string>();
    const refresh = async () => {
      await refreshAgentRuntime();
      if (!active) return;
      if (!migrated) {
        const outcome = await migrateLegacyTrees(user.id);
        // Blocked (no daemon yet, another account): nothing happened, so try
        // again on the next refresh — set-up may finish meanwhile. A tree that
        // failed half-way waits for the next launch rather than retrying every
        // minute.
        if (outcome.status !== "blocked") migrated = true;
        if (outcome.status === "done" && outcome.failed.length) {
          console.warn("Moving to Pi Dash on this computer did not finish for", outcome.failed);
        }
      }
      // "Is this <label>? Replace it": once per launch for each machine the
      // daemon is signed in as (a fresh sign-in is a new machine id).
      const machineId = compatibleHello()?.machine_id;
      if (!active || !migrated || !machineId || askedFor.has(machineId)) return;
      askedFor.add(machineId);
      // A failed lookup (offline) is not an answer: ask again next refresh.
      await offerReplace(user.id).catch(() => {
        askedFor.delete(machineId);
      });
    };
    const tick = () => void refresh().catch(() => {});
    tick();
    const timer = setInterval(tick, 60_000);
    window.addEventListener("focus", tick);
    return () => {
      active = false;
      clearInterval(timer);
      window.removeEventListener("focus", tick);
    };
  }, [user?.id, user?.email]);

  if (!isDesktop()) return null;
  return (
    <>
      {/* Mounted on every page: sign-out and set-up can start from any of them. */}
      <DesktopSignOutDialog />
      <DesktopDaemonSetupDialog />
      <DesktopReplaceMachineDialog />
      {user?.id && <DesktopMigrationNotices account={user.id} />}
    </>
  );
});
