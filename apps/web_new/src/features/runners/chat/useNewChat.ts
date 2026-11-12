// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// "New chat" (NEWFRONT-549, RUN-026). Creation shows a working state,
// selects the session before the history list is updated so no render can
// resolve the previous conversation, and leaves the list untouched when
// the server refuses. Failures surface a toast; the caller carries the
// selection into the address. The page's warm-up owns preparing the
// created session, so this hook never warms one itself (RUN-028).

import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { ChatSession } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { chatKeys } from "./keys.js";
import { openChatSession } from "./transport.js";

/** Toast headline when a new conversation cannot be opened. */
export const NEW_CHAT_FAILED_TITLE = "Unable to start chat";
export const NEW_CHAT_FAILED_BODY = "Could not open a new conversation";

export interface NewChatInput {
  workspaceSlug: string;
  workspaceId: string;
  runnerId: string;
  /** Selection first, then the cache: order matters for warm-up. */
  onSelect: (sessionId: string) => void;
  /** Told about the created row so the page can resolve it immediately. */
  onCreated: (session: ChatSession) => void;
  /** A create is already running (the control is disabled). */
  busy: boolean;
}

export interface NewChatHandle {
  busy: boolean;
  start: () => Promise<void>;
}

/** True for a reply the API or a transport could not deliver. */
export function chatFailureReason(error: unknown): string {
  if (error instanceof Error && error.message !== "") return error.message;
  return NEW_CHAT_FAILED_BODY;
}

export function useNewChat({
  workspaceSlug,
  workspaceId,
  runnerId,
  onSelect,
  onCreated,
  busy,
}: NewChatInput): NewChatHandle {
  const queryClient = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const inFlightRef = React.useRef(false);
  const liveRunnerRef = React.useRef(runnerId);
  liveRunnerRef.current = runnerId;

  // A different contact starts with a clean slate: a create still running
  // for the contact the viewer left can no longer report its state here.
  React.useEffect(() => {
    inFlightRef.current = false;
    setCreating(false);
  }, [runnerId]);

  const start = React.useCallback(async (): Promise<void> => {
    if (busy || inFlightRef.current) return;
    inFlightRef.current = true;
    setCreating(true);
    try {
      const created = await openChatSession(getAppClient(), { workspace: workspaceId, runner: runnerId });
      // The viewer moved to another contact while the create was in
      // flight: the session belongs to the contact no longer on screen,
      // so it must not be published, selected or remembered here.
      if (liveRunnerRef.current !== runnerId) return;
      onCreated(created);
      queryClient.setQueryData<ChatSession[]>(chatKeys.sessions(workspaceSlug, runnerId), (current) => {
        const existing = current ?? [];
        return existing.some((item) => item.id === created.id) ? existing : [created, ...existing];
      });
      onSelect(created.id);
    } catch (error) {
      if (liveRunnerRef.current !== runnerId) return;
      notifyError(NEW_CHAT_FAILED_TITLE, chatFailureReason(error));
    } finally {
      if (liveRunnerRef.current === runnerId) {
        inFlightRef.current = false;
        setCreating(false);
      }
    }
  }, [busy, onCreated, onSelect, queryClient, runnerId, workspaceId, workspaceSlug]);

  return { busy: busy || creating, start };
}
