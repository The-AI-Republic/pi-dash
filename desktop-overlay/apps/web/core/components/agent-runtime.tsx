/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import { useMatches } from "react-router";
import { useUser } from "@/hooks/store/user";
import { useProject } from "@/hooks/store/use-project";
import { connectAgentProject, isDesktop, refreshAgentRuntime, resumeAgentRuntime } from "@/services/agent-runtime";

export const AgentRuntime = observer(function AgentRuntime() {
  const { data: user } = useUser();
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

  useEffect(() => {
    if (!isDesktop() || !user?.id) return;
    resumeAgentRuntime(user.id);
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
    const refreshProject = async () => {
      if (!active || !enrolled || refreshing) return;
      refreshing = true;
      try {
        // Update the same observable project used by the executor picker.
        // Enrollment can finish before the daemon's first heartbeat.
        const project = await fetchProjectDetails(workspaceSlug, projectId);
        if (active)
          setMessage(
            project.agent_executor_options?.some((option) => option.kind === "managed_runner" && option.available)
              ? "Runs on this computer · Pi Dash uses its own working copy. Keep the app open while the agent runs."
              : "Waiting for Pi Dash Agent to connect…"
          );
      } catch (error) {
        if (active) setMessage(String(error instanceof Error ? error.message : error));
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
        }
      );
    };
    const timer = setInterval(() => void refreshProject(), 5000);
    window.addEventListener("focus", reconnect);
    return () => {
      active = false;
      clearInterval(timer);
      window.removeEventListener("focus", reconnect);
    };
  }, [user?.id, workspaceSlug, projectId, fetchProjectDetails]);

  if (!isDesktop() || !user?.id || !projectId || !message) return null;
  return (
    <div
      role="status"
      className="shadow-sm fixed bottom-2 left-1/2 z-20 max-w-xl -translate-x-1/2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-body-xs-regular"
    >
      {message}
    </div>
  );
});
