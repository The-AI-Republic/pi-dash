// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Background agent-runtime supervisor (RUN-044) with the per-project
// enrollment pill (RUN-045). Desktop only: on web and cloud the platform
// exposes no runtime capability, so this renders nothing and runs no
// step. Mounted once in the signed-in workspace layout. On desktop it
// resumes the runtime for the signed account, registers the local chat
// transport, refreshes the engine setup on a minute cadence plus window
// focus, and enrolls each opened project while a pill tracks preparing,
// waiting and runs-locally (or the honest failure).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import type { Project } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { useMe } from "../../../core/session/index.js";
import { agentFailureMessage } from "../../../shared/lib/agentReasons.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { isManagedRunnerAvailable, projectAvailabilityQueryOptions, runtimeKeys } from "./queries.js";
import { connectProject, refreshRuntime, resumeRuntime } from "./service.js";

/** Background engine-setup refresh cadence (plus every window focus). */
export const RUNTIME_REFRESH_INTERVAL_MS = 60_000;

/** Managed-runner availability poll while the pill waits. */
export const AVAILABILITY_POLL_INTERVAL_MS = 5_000;

export const RUNTIME_PREPARING_TEXT = "Getting the local agent ready…";
export const RUNTIME_WAITING_TEXT = "Waiting for the local agent to connect…";
export const RUNTIME_CONNECTED_TEXT = "This project runs on this computer. Keep the app open while the agent runs.";

export interface RuntimeNoticeInput {
  connectError: string | null;
  availabilityError: string | null;
  enrolled: boolean;
  available: boolean;
  refreshError: string | null;
}

/**
 * Pick the pill text: a failed enrollment or poll first (honest errors,
 * never a false claim), then the enrollment phase, with a background
 * refresh failure surfacing once connected.
 */
export function selectRuntimeNotice(input: RuntimeNoticeInput): string {
  if (input.connectError) return input.connectError;
  if (input.availabilityError) return input.availabilityError;
  if (!input.enrolled) return RUNTIME_PREPARING_TEXT;
  if (!input.available) return RUNTIME_WAITING_TEXT;
  return input.refreshError ?? RUNTIME_CONNECTED_TEXT;
}

/**
 * Notice text for a failed step. A server refusal is rendered through the
 * shared agent vocabulary, so the reason code becomes a plain-language
 * explanation of that specific refusal; anything else keeps its message.
 */
function errorMessage(error: unknown): string {
  return agentFailureMessage(error);
}

/**
 * Resolve a direct work-item link (IDENTIFIER-sequence) to its project by
 * identifier within the workspace collection. Unknown links resolve to
 * nothing and enroll nothing.
 */
export function matchProjectIdentifier(
  projects: Array<Project> | undefined,
  issueKey: string | undefined
): string | undefined {
  if (!projects || !issueKey) return undefined;
  const identifier = issueKey.split("-")[0];
  return projects.find((project) => project.identifier === identifier)?.id;
}

export interface RuntimeSupervisorProps {
  /** Registers the local chat transport once the runtime resumes (desktop chat slice). */
  registerChatTransport?: (() => void) | undefined;
}

interface SupervisorRouteParams {
  ws?: string | undefined;
  projectId?: string | undefined;
  issueKey?: string | undefined;
}

/** Install the local chat transport once its chunk has loaded. */
function registerLocalChatTransportLazily(): void {
  void import("../chat/localBootstrap.js").then(
    (module) => module.registerLocalChatTransport(),
    () => undefined
  );
}

export function RuntimeSupervisor(props: RuntimeSupervisorProps): React.ReactElement | null {
  // The desktop chat transport is loaded on demand: the supervisor itself
  // rides the first-paint chain, and most builds never take this path.
  const registerChatTransport = props.registerChatTransport ?? registerLocalChatTransportLazily;
  const client = getAppClient();
  const queryClient = useQueryClient();
  const runtime = platform.agentRuntime;
  const { data: me } = useMe();
  const userId = me?.id;
  const params = useParams({ strict: false }) as SupervisorRouteParams;
  const ws = params.ws;
  const routeProjectId = params.projectId;
  // Direct work-item links carry no project id: resolve the owning project
  // by identifier within the workspace collection before enrolling
  // anything. The shell already loads the collection, so this reuses its
  // cached row instead of fetching.
  const issueKey = routeProjectId ? undefined : params.issueKey;
  const projects = useQuery({
    ...projectsQueryOptions(client, ws ?? ""),
    enabled: !!runtime && !!ws && !!issueKey,
  });
  const projectId = routeProjectId ?? matchProjectIdentifier(projects.data, issueKey) ?? undefined;

  const [enrolled, setEnrolled] = React.useState(false);
  const [connectError, setConnectError] = React.useState<string | null>(null);
  const [refreshError, setRefreshError] = React.useState<string | null>(null);

  const availability = useQuery({
    ...projectAvailabilityQueryOptions(client, ws ?? "", projectId ?? ""),
    enabled: !!runtime && enrolled && !!ws && !!projectId,
    // The poll only waits for the first heartbeat: the interval stops as
    // soon as the cached row reports the agent available. Leaving the
    // project stops it via the enabled flag; a focus-triggered reconnect
    // re-reads the row, restarting the wait when the agent went away.
    refetchInterval: (query) =>
      query.state.error || !isManagedRunnerAvailable(query.state.data) ? AVAILABILITY_POLL_INTERVAL_MS : false,
  });
  const available = isManagedRunnerAvailable(availability.data);
  const availabilityError = availability.isError ? errorMessage(availability.error) : null;
  const notice = selectRuntimeNotice({ connectError, availabilityError, enrolled, available, refreshError });

  React.useEffect(() => {
    if (!runtime || !userId) return;
    resumeRuntime(userId, { client, runtime });
    registerChatTransport?.();
    let disposed = false;
    const refresh = (): void => {
      void refreshRuntime({ client, runtime }).then(
        () => {
          if (!disposed) setRefreshError(null);
        },
        (error: unknown) => {
          if (!disposed) setRefreshError(errorMessage(error));
        }
      );
    };
    const timer = setInterval(refresh, RUNTIME_REFRESH_INTERVAL_MS);
    const unsubscribe = platform.onFocusChange((focused) => {
      if (focused) refresh();
    });
    return () => {
      disposed = true;
      clearInterval(timer);
      unsubscribe();
    };
  }, [runtime, userId, client, registerChatTransport]);

  React.useEffect(() => {
    setEnrolled(false);
    setConnectError(null);
    if (!runtime || !userId || !ws || !projectId) return;
    let active = true;
    resumeRuntime(userId, { client, runtime });
    const reconnect = (): void => {
      setConnectError(null);
      void connectProject({ client, runtime, workspaceSlug: ws, projectId }).then(
        () => {
          if (!active) return;
          setEnrolled(true);
          // Re-read availability immediately, then let the interval carry
          // on. A focus-triggered reconnect restarts the wait when the
          // agent went away, since the interval follows the cached row.
          void queryClient.invalidateQueries({ queryKey: runtimeKeys.projectAvailability(ws, projectId) });
        },
        (error: unknown) => {
          if (active) setConnectError(errorMessage(error));
        }
      );
    };
    reconnect();
    const unsubscribe = platform.onFocusChange((focused) => {
      if (focused) reconnect();
    });
    return () => {
      active = false;
      unsubscribe();
    };
  }, [runtime, userId, ws, projectId, client, queryClient]);

  if (!runtime) return null;
  return userId && projectId ? (
    <div
      role="status"
      data-testid="runtime-notice"
      className="text-caption fixed bottom-(--space-4) left-1/2 z-40 w-max max-w-[min(36rem,92vw)] -translate-x-1/2 rounded-(--radius-full) border border-(--border) bg-(--surface) px-(--space-6) py-(--space-2) text-(--text)"
    >
      {notice}
    </div>
  ) : null;
}
