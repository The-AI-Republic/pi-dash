// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session warm-up on open (NEWFRONT-549, RUN-028). Opening a chat prepares
// a conversation so the first turn streams promptly: an open session on
// screen is warmed, and when nothing is selected an empty session is
// created first so the composer has a target. A created session is
// published to the history list before the list refetches, and the work
// is de-duplicated so a re-render (or a refetch that hands back an equal
// but new object) cannot warm or create twice.

import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { ChatSession, Runner } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { chatKeys } from "./keys.js";
import { openChatSession, prepareChatSession } from "./transport.js";
import { warmUpStep, type WarmUpStep } from "./chatState.js";

export interface WarmUpInput {
  workspaceSlug: string;
  workspaceId: string;
  runnerId: string;
  runner: Pick<Runner, "status" | "enrolled_at"> | null | undefined;
  sessions: ChatSession[] | undefined;
  session: ChatSession | null;
  pendingId: string | null;
  requestedId: string | null;
  /** The area admitted the viewer; denied pages never warm. */
  enabled: boolean;
  /** Receives a session created here before the list refetch lands. */
  onCreated: (session: ChatSession) => void;
}

/** Prepare one session, tolerating a cold start that fails. */
async function warmQuietly(sessionId: string): Promise<void> {
  try {
    await prepareChatSession(getAppClient(), sessionId);
  } catch {
    /* Warm-up only saves latency; the session stays usable. */
  }
}

/**
 * Runs the warm-up decision for the current runner and conversation.
 * A closed history session the viewer picked is never replaced; a stale
 * address (no matching session) still auto-creates on a chat-capable
 * runner. Answers `true` while a create is outstanding.
 */
export function useWarmUp(input: WarmUpInput): boolean {
  const queryClient = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const warmedRef = React.useRef<string | null>(null);
  const createdForKeyRef = React.useRef<string | null>(null);
  const liveRunnerRef = React.useRef(input.runnerId);
  liveRunnerRef.current = input.runnerId;

  const onCreatedRef = React.useRef(input.onCreated);
  onCreatedRef.current = input.onCreated;

  const { workspaceSlug, workspaceId, runnerId, enabled } = input;
  const runnerStatus = input.runner?.status ?? null;
  const enrolledAt = input.runner?.enrolled_at ?? null;
  const sessionId = input.session?.id ?? null;
  const sessionStatus = input.session?.status ?? null;
  const sessionsKnown = input.sessions !== undefined;
  const { pendingId, requestedId } = input;

  const step: WarmUpStep = React.useMemo(() => {
    const runner = runnerStatus === null ? null : { status: runnerStatus, enrolled_at: enrolledAt };
    return warmUpStep({
      runner,
      sessionsKnown,
      session: sessionId === null ? null : { id: sessionId, status: sessionStatus ?? "open" },
      pendingId,
      requestedId,
    });
  }, [enrolledAt, pendingId, requestedId, runnerStatus, sessionId, sessionStatus, sessionsKnown]);

  React.useEffect(() => {
    // A different contact starts with a clean slate.
    warmedRef.current = null;
    createdForKeyRef.current = null;
    setCreating(false);
  }, [runnerId]);

  React.useEffect(() => {
    if (!enabled) return;

    if (step.kind === "warm") {
      if (warmedRef.current === step.sessionId) return;
      warmedRef.current = step.sessionId;
      void warmQuietly(step.sessionId);
      return;
    }
    if (step.kind !== "create") return;

    const createKey = `${workspaceId}:${runnerId}`;
    if (createdForKeyRef.current === createKey) return;
    createdForKeyRef.current = createKey;
    setCreating(true);
    void (async () => {
      let created: ChatSession;
      try {
        created = await openChatSession(getAppClient(), { workspace: workspaceId, runner: runnerId });
      } catch {
        // Leave the history as-is; the next decision (or New chat) retries.
        if (liveRunnerRef.current === runnerId) setCreating(false);
        return;
      }
      // The viewer moved to another contact while the create was in flight:
      // the stale session belongs to nobody on screen, so drop it silently.
      if (liveRunnerRef.current !== runnerId) return;
      warmedRef.current = created.id;
      onCreatedRef.current(created);
      queryClient.setQueryData<ChatSession[]>(chatKeys.sessions(workspaceSlug, runnerId), (current) => {
        const existing = current ?? [];
        return existing.some((item) => item.id === created.id) ? existing : [created, ...existing];
      });
      await warmQuietly(created.id);
      if (liveRunnerRef.current !== runnerId) return;
      setCreating(false);
      void queryClient.invalidateQueries({ queryKey: chatKeys.all(workspaceSlug) });
    })();
  }, [enabled, queryClient, runnerId, step, workspaceId, workspaceSlug]);

  return creating;
}
