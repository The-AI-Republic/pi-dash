/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useRef, useState } from "react";
import { observer } from "mobx-react";
import { useMatches } from "react-router";
import { useUser } from "@/hooks/store/user";
import { useProject } from "@/hooks/store/use-project";
import { DesktopAccountSwitchDialog } from "@/components/desktop-account-switch-dialog";
import { DesktopDaemonNotice } from "@/components/desktop-daemon-notice";
import { DesktopSignOutDialog } from "@/components/desktop-sign-out-dialog";
import {
  connectAgentProject,
  isDesktop,
  refreshAgentRuntime,
  resumeAgentRuntime,
  signOutFollowingCli,
} from "@/services/agent-runtime";
import { ensureSharedDaemon, syncAccounts } from "@/services/desktop-account-sync";
import { registerLocalChatTransport } from "@/services/local-chat-transport";

/** How often account sync re-checks the CLI's sign-in while the app is open. */
const ACCOUNT_SYNC_INTERVAL_MS = 15_000;

export const AgentRuntime = observer(function AgentRuntime() {
  const userStore = useUser();
  const { data: user } = userStore;
  const { fetchProjectDetails, workspaceProjectIds, getProjectById } = useProject();
  // `useParams()` resolves against the route that renders the component, and
  // this one is mounted on the root route (`app/provider.tsx`, inside
  // `root.tsx`'s `<AppProvider>`). The root route declares no path params, so
  // `useParams()` would return `{}` on every page and the project effect below
  // would never run. `useMatches()` reads the whole matched chain from the
  // router state instead; the leaf match carries the accumulated params.
  const matches = useMatches();
  const {
    workspaceSlug,
    projectId: routeProjectId,
    workItem,
  } = (matches[matches.length - 1]?.params ?? {}) as {
    workspaceSlug?: string;
    projectId?: string;
    workItem?: string;
  };
  // Direct /browse/IDENTIFIER-123 links have no projectId route parameter.
  // Resolve only within the active workspace: identifiers can repeat elsewhere.
  const projectId =
    routeProjectId ??
    (workItem
      ? workspaceProjectIds?.find((id) => getProjectById(id)?.identifier === workItem.replace(/-\d+$/, ""))
      : undefined);
  const [message, setMessage] = useState("");

  // Latest values for the account-sync loop, which outlives renders.
  const syncContext = useRef({ userId: user?.id, email: user?.email, workspaceSlug, signOut: userStore.signOut });
  syncContext.current = { userId: user?.id, email: user?.email, workspaceSlug, signOut: userStore.signOut };

  // The one `pidash` daemon this computer shares with the CLI: install the
  // CLI and start the daemon on launch, then keep the app's sign-in and the
  // CLI's in step — on launch, on focus and every 15 s. Runs signed out too:
  // that is when a CLI sign-in is handed to the app.
  useEffect(() => {
    if (!isDesktop()) return;
    let active = true;
    const sync = () => {
      if (!active) return;
      const { userId, email, workspaceSlug: slug, signOut } = syncContext.current;
      void syncAccounts({
        desktopUserId: userId,
        desktopEmail: email,
        workspaceSlug: slug,
        signOutDesktop: () => signOutFollowingCli(() => signOut({ skipConfirmation: true })),
      }).catch((error: unknown) => {
        console.warn("Pi Dash account sync failed:", error);
      });
    };
    void ensureSharedDaemon()
      .catch((error: unknown) => console.warn("Pi Dash daemon start failed:", error))
      .finally(sync);
    const timer = setInterval(sync, ACCOUNT_SYNC_INTERVAL_MS);
    window.addEventListener("focus", sync);
    return () => {
      active = false;
      clearInterval(timer);
      window.removeEventListener("focus", sync);
    };
  }, []);

  useEffect(() => {
    if (!isDesktop() || !user?.id) return;
    resumeAgentRuntime(user.id);
    // Point the shared chat UI at the bundled engine (local Tauri IPC) instead
    // of the cloud chat relay. Idempotent and account-agnostic — the transport
    // reads the current account per call — so a single registration survives
    // account switches.
    registerLocalChatTransport();
    const refresh = () => {
      void refreshAgentRuntime().catch((error: unknown) => {
        setMessage(String(error instanceof Error ? error.message : error));
      });
    };
    const timer = setInterval(refresh, 60_000);
    window.addEventListener("focus", refresh);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", refresh);
    };
  }, [user?.id]);

  useEffect(() => {
    if (!isDesktop() || !user?.id || !workspaceSlug || !projectId) return;
    let active = true;
    let enrolled = false;
    let refreshing = false;
    let timer: ReturnType<typeof setInterval> | undefined;
    const stopPolling = () => {
      clearInterval(timer);
      timer = undefined;
    };
    const startPolling = () => {
      if (active && timer === undefined) timer = setInterval(() => void refreshProject(), 5000);
    };
    const refreshProject = async () => {
      if (!active || !enrolled || refreshing) return;
      refreshing = true;
      try {
        // Update the same observable project used by the executor picker.
        // Enrollment can finish before the daemon's first heartbeat.
        const project = await fetchProjectDetails(workspaceSlug, projectId);
        if (!active) return;
        const available = project.agent_executor_options?.some(
          (option) => option.kind === "managed_runner" && option.available
        );
        // The poll only waits for that first heartbeat. Once the agent is
        // available there is nothing left to wait for; the focus re-check
        // below restarts it if the agent has gone away since.
        if (available) stopPolling();
        else startPolling();
        setMessage(
          available
            ? "Runs on this computer · Pi Dash uses its own working copy."
            : "Waiting for Pi Dash Agent to connect…"
        );
      } catch (error) {
        if (active) setMessage(String(error instanceof Error ? error.message : error));
        // A failed re-check must not strand the strip on the error.
        startPolling();
      } finally {
        refreshing = false;
      }
    };
    resumeAgentRuntime(user.id);
    setMessage("Preparing Pi Dash Agent…");
    void connectAgentProject(workspaceSlug, projectId).then(
      // Terminal handler: the chain ends here (`void`), and enrollment reports
      // through setMessage rather than a resolved value.
      // eslint-disable-next-line promise/always-return
      async () => {
        enrolled = true;
        await refreshProject();
      },
      (error: unknown) => {
        if (active) setMessage(String(error instanceof Error ? error.message : error));
      }
    );
    const reconnect = () => {
      void connectAgentProject(workspaceSlug, projectId).then(
        // eslint-disable-next-line promise/always-return -- terminal handler, as above.
        async () => {
          enrolled = true;
          await refreshProject();
        },
        (error: unknown) => {
          if (active) setMessage(String(error instanceof Error ? error.message : error));
          startPolling();
        }
      );
    };
    startPolling();
    window.addEventListener("focus", reconnect);
    return () => {
      active = false;
      stopPolling();
      window.removeEventListener("focus", reconnect);
    };
  }, [user?.id, workspaceSlug, projectId, fetchProjectDetails]);

  if (!isDesktop()) return null;
  return (
    <>
      {/* Mounted on every page: sign-out can be started from any of them. */}
      <DesktopSignOutDialog />
      <DesktopDaemonNotice />
      <DesktopAccountSwitchDialog />
      {user?.id && projectId && message && (
        <div
          role="status"
          className="shadow-sm fixed bottom-2 left-1/2 z-20 max-w-xl -translate-x-1/2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-body-xs-regular"
        >
          {message}
        </div>
      )}
    </>
  );
});
