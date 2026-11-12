// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// On-machine history writes (NEWFRONT-589, DESK-009): rename, delete,
// clear and the working-copy read. Every verb goes through the chat
// transport seam, so only the build that carries the bundled engine can
// perform one; the web bundle has no such transport and the controls are
// never rendered there. The list and the transcript are re-read on settle,
// because the store owns both and a failed write must leave them as the
// store has them, not as the click assumed.

import { useMutation, useQueryClient } from "@tanstack/react-query";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { chatKeys } from "./keys.js";
import { chatTransportForRunner, clearChatHistory, deleteChatSession, renameChatSession } from "./transport.js";

export const RENAME_FAILED_TITLE = "Chat not renamed";
export const DELETE_FAILED_TITLE = "Chat not deleted";
export const CLEAR_FAILED_TITLE = "History not cleared";
export const RENAMED_TOAST = "Chat renamed";
export const DELETED_TOAST = "Chat deleted";
export const CLEARED_TOAST = "History cleared";
export const HISTORY_FAILED_BODY = "The change did not reach this computer's chat store.";

/** Whether this surface keeps chat history on the machine at all. */
export function supportsLocalHistory(runnerId: string): boolean {
  return chatTransportForRunner(runnerId).deleteChatSession !== undefined;
}

/** Rename one conversation, then re-read the runner's history. */
export function useRenameChat(workspaceSlug: string, runnerId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { sessionId: string; title: string }>({
    mutationFn: async (input) => {
      const changed = await renameChatSession(input.sessionId, input.title);
      if (!changed) throw new Error(HISTORY_FAILED_BODY);
    },
    onSuccess: () => notifySuccess(RENAMED_TOAST),
    onError: (error) => notifyError(RENAME_FAILED_TITLE, reasonOf(error)),
    onSettled: () => queryClient.invalidateQueries({ queryKey: chatKeys.sessions(workspaceSlug, runnerId) }),
  });
}

/** Delete one conversation with its messages; a repeat click settles quietly. */
export function useDeleteChat(workspaceSlug: string, runnerId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { sessionId: string }>({
    mutationFn: async (input) => {
      const changed = await deleteChatSession(input.sessionId);
      if (!changed) throw new Error(HISTORY_FAILED_BODY);
    },
    onSuccess: () => notifySuccess(DELETED_TOAST),
    onError: (error) => notifyError(DELETE_FAILED_TITLE, reasonOf(error)),
    onSettled: (_data, _error, input) =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: chatKeys.sessions(workspaceSlug, runnerId) }),
        queryClient.removeQueries({ queryKey: chatKeys.messages(workspaceSlug, input.sessionId) }),
      ]),
  });
}

/** Wipe the signed-in account's whole on-machine history. */
export function useClearHistory(workspaceSlug: string, runnerId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: async () => {
      const changed = await clearChatHistory();
      if (!changed) throw new Error(HISTORY_FAILED_BODY);
    },
    onSuccess: () => notifySuccess(CLEARED_TOAST),
    onError: (error) => notifyError(CLEAR_FAILED_TITLE, reasonOf(error)),
    onSettled: () => queryClient.invalidateQueries({ queryKey: chatKeys.sessions(workspaceSlug, runnerId) }),
  });
}

function reasonOf(error: unknown): string {
  if (error instanceof Error && error.message !== "") return error.message;
  return HISTORY_FAILED_BODY;
}
