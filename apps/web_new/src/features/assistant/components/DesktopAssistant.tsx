// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop assistant (NEWPIDASH-300): the Assistant page through the local
// Pi Dash Agent. On a build with the local chat capability the page mounts
// the runner chat for the bundled agent instead of the server surfaces, so
// every turn runs on this machine through the daemon and no assistant turn
// reaches the server. The runners area's Pi Dash Agent contact and this
// page share one transport and one local store — the same sessions, the
// same transcript — and the availability read is the same one the rail
// uses (the desktop profile endpoint is never called off the desktop,
// AGT-059). When the agent cannot run, the page says why with the server's
// reason sentence; it never falls back to the cloud assistant.

import { getPidashAgentProfile } from "@pidash/api-client";
import { Spinner } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import { Bot } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { selectWorkspace, usePermissions, useWorkspaces } from "../../../core/session/index.js";
import {
  BUILTIN_CHAT_ID,
  buildRunnersAreaValue,
  decideWorkspaceRunnersAccess,
  LOCAL_AGENT_POLL_INTERVAL_MS,
  LOCAL_AGENT_UNAVAILABLE_FALLBACK,
  localChatQueryKey,
  pidashAgentContactAvailability,
  RunnerChatPage,
  RunnersAreaProvider,
  RunnersNotAuthorized,
} from "../../runners/index.js";
import { DESKTOP_AGENT_UNAVAILABLE_TITLE } from "../lib/copy.js";

export interface DesktopAssistantProps {
  workspaceSlug: string;
  /** Local session id from the thread address, or null on the landing. */
  requestedSessionId: string | null;
  /** A history pick rewrites the thread address; the page re-reads it. */
  onSelectSession: (sessionId: string) => void;
}

export function DesktopAssistant({
  workspaceSlug,
  requestedSessionId,
  onSelectSession,
}: DesktopAssistantProps): React.ReactElement | null {
  // The availability read the runners rail's contact uses, on the same
  // cache key: one server answer serves both surfaces.
  const availability = useQuery({
    queryKey: localChatQueryKey(),
    queryFn: () => getPidashAgentProfile(getAppClient()),
    staleTime: LOCAL_AGENT_POLL_INTERVAL_MS,
    refetchInterval: LOCAL_AGENT_POLL_INTERVAL_MS,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const workspace = selectWorkspace(useWorkspaces().data, workspaceSlug);
  const permissions = usePermissions(workspaceSlug);
  const access = decideWorkspaceRunnersAccess({ role: permissions.role, isLoading: permissions.isLoading });

  // The workspace slice and the role records settle before the page
  // renders at all, exactly like the runners routes (RUN-001).
  if (availability.isPending || workspace === undefined || access === "loading") {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label="Loading assistant" />
      </div>
    );
  }
  // The same agent obeys the same workspace rule in both areas.
  if (access === "denied") return <RunnersNotAuthorized scope="workspace" />;

  let reason: string | null = null;
  if (availability.isError) {
    const error = availability.error;
    reason = error instanceof Error && error.message !== "" ? error.message : LOCAL_AGENT_UNAVAILABLE_FALLBACK;
  } else if (availability.data !== undefined) {
    reason = pidashAgentContactAvailability(availability.data).unavailableReason;
  }
  if (reason !== null) return <DesktopUnavailable reason={reason} />;

  const area = buildRunnersAreaValue({ workspaceSlug }, workspace.id, access);
  return (
    <RunnersAreaProvider value={area}>
      <RunnerChatPage
        workspaceSlug={workspaceSlug}
        runnerId={BUILTIN_CHAT_ID}
        requestedSessionId={requestedSessionId}
        onSelectSession={onSelectSession}
      />
    </RunnersAreaProvider>
  );
}

/**
 * The desktop panel for an agent that cannot run: the product name and
 * the server's reason sentence, and nothing else. There is no cloud
 * fallback behind it.
 */
function DesktopUnavailable({ reason }: { reason: string }): React.ReactElement {
  return (
    <div className="flex h-full w-full items-center justify-center px-(--space-4)">
      <section
        aria-label={DESKTOP_AGENT_UNAVAILABLE_TITLE}
        data-testid="assistant-desktop-unavailable"
        className="mx-auto flex w-full max-w-md flex-col items-center gap-(--space-3) rounded-(--radius-dialog) border border-(--border) bg-(--surface) px-(--space-6) py-(--space-8) text-center"
      >
        <Bot aria-hidden="true" size={22} className="text-(--accent)" />
        <h2 className="text-h3 text-(--text)">{DESKTOP_AGENT_UNAVAILABLE_TITLE}</h2>
        <p data-testid="assistant-desktop-unavailable-reason" className="text-body text-(--text-muted)">
          {reason}
        </p>
      </section>
    </div>
  );
}
