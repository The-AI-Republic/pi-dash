// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat write verbs (NEWFRONT-551, RUN-027/030/031). Sending reuses the open
// conversation or opens one, clears the draft before the round-trip and
// puts it back together with its reason when the server refuses. Stop
// interrupts the running turn and drops any parked approval; close shuts
// the conversation and lets the history speak for itself. Each verb
// refetches only what the old page did: a send re-reads messages and
// sessions, stop and close re-read sessions.
import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { createChatSession, type ChatSession } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { chatKeys } from "./keys.js";
import { getChatTransport } from "./transport.js";

/** Toast headline when the server will not take a message. */
export const SEND_FAILED_TITLE = "Message not sent";
/** Reason shown when a failure carries nothing readable. */
export const SEND_FAILED_BODY = "The message could not be sent";

/** The reason to show for a refused or dropped send. */
export function chatFailureReason(error: unknown): string {
  if (error instanceof Error && error.message !== "") return error.message;
  if (typeof error === "string" && error !== "") return error;
  return SEND_FAILED_BODY;
}

export interface ChatActionInput {
  workspaceSlug: string;
  workspaceId: string;
  runnerId: string;
  /** The conversation on screen, if one is resolved. */
  session: ChatSession | null;
  /** Selects a conversation the action opened. */
  onSelectSession: (sessionId: string) => void;
  /** Tells the page about the created row before the list answers. */
  onCreated: (session: ChatSession) => void;
}

export interface ChatActionHandle {
  /** A send is in flight; the composer stays shut until it lands. */
  sending: boolean;
  /** A send failed: the reason stays above the composer until dismissed. */
  sendError: string | null;
  dismissSendError: () => void;
  send: (draft: string, onFailure: (draft: string) => void) => Promise<void>;
  stop: () => Promise<void>;
  close: () => Promise<void>;
}

export function useChatActions({
  workspaceSlug,
  workspaceId,
  runnerId,
  session,
  onSelectSession,
  onCreated,
}: ChatActionInput): ChatActionHandle {
  const queryClient = useQueryClient();
  const [sending, setSending] = React.useState(false);
  const [sendError, setSendError] = React.useState<string | null>(null);
  const inFlightRef = React.useRef(false);
  const liveRunnerRef = React.useRef(runnerId);
  liveRunnerRef.current = runnerId;

  // The conversation the send targets, refreshed on every render.
  const sessionRef = React.useRef(session);
  sessionRef.current = session;

  React.useEffect(() => {
    inFlightRef.current = false;
    setSending(false);
    setSendError(null);
  }, [runnerId]);

  const publishSession = React.useCallback(
    (created: ChatSession) => {
      onCreated(created);
      queryClient.setQueryData<ChatSession[]>(chatKeys.sessions(workspaceSlug, runnerId), (current) => {
        const existing = current ?? [];
        return existing.some((item) => item.id === created.id) ? existing : [created, ...existing];
      });
    },
    [onCreated, queryClient, runnerId, workspaceSlug]
  );

  const ensureSession = React.useCallback(async (): Promise<ChatSession> => {
    const current = sessionRef.current;
    if (current !== null && current.status === "open") return current;
    const created = await createChatSession(getAppClient(), { workspace: workspaceId, runner: runnerId });
    publishSession(created);
    return created;
  }, [publishSession, runnerId, workspaceId]);

  const send = React.useCallback(
    async (draft: string, onFailure: (draft: string) => void): Promise<void> => {
      const content = draft.trim();
      if (content === "" || inFlightRef.current) return;
      inFlightRef.current = true;
      setSending(true);
      setSendError(null);
      try {
        const target = await ensureSession();
        // The viewer left this contact while the conversation opened: the
        // message belongs to the contact no longer on screen.
        if (liveRunnerRef.current !== runnerId) return;
        if (target.id !== sessionRef.current?.id) onSelectSession(target.id);
        const transport = getChatTransport();
        // The desktop transport hands the runner's approval mode to the
        // engine here; the cloud transport has no such step.
        transport.applyApprovalMode?.(target.id);
        await transport.sendChatMessage(target.id, content);
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: chatKeys.messages(workspaceSlug, target.id) }),
          queryClient.invalidateQueries({ queryKey: chatKeys.sessions(workspaceSlug, runnerId) }),
        ]);
      } catch (error) {
        if (liveRunnerRef.current !== runnerId) return;
        const reason = chatFailureReason(error);
        // The draft goes back exactly as typed, beside the reason.
        onFailure(draft);
        setSendError(reason);
        notifyError(SEND_FAILED_TITLE, reason);
      } finally {
        if (liveRunnerRef.current === runnerId) {
          inFlightRef.current = false;
          setSending(false);
        }
      }
    },
    [ensureSession, onSelectSession, queryClient, runnerId, workspaceSlug]
  );

  const stop = React.useCallback(async (): Promise<void> => {
    const current = sessionRef.current;
    if (current === null) return;
    // Interrupting answers any parked approval, so it is dropped at once
    // rather than left as a button that can no longer do anything.
    setSendError(null);
    await getChatTransport().cancelChat(current.id, "user_cancelled");
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(workspaceSlug, runnerId) });
  }, [queryClient, runnerId, workspaceSlug]);

  const close = React.useCallback(async (): Promise<void> => {
    const current = sessionRef.current;
    if (current === null) return;
    await getChatTransport().closeChat(current.id);
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(workspaceSlug, runnerId) });
  }, [queryClient, runnerId, workspaceSlug]);

  const dismissSendError = React.useCallback(() => setSendError(null), []);

  return { sending, sendError, dismissSendError, send, stop, close };
}
