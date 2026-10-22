// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI-assistant settings writes (parity/specs/profile.md §10, PROF-021..023).
// Each operation reports through the shell's notices on its own so a
// failed write keeps the form exactly as typed; the cached read is
// replaced by the server's answer (or re-read after a removal).

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createMcpServer,
  deleteLlmConfig,
  deleteMcpServer,
  deleteSttConfig,
  dictationDisabledFrom,
  patchMcpServer,
  putLlmConfig,
  putSttConfig,
  testLlmConfig,
  testSttConfig,
  type LlmConfig,
  type LlmConfigInput,
  type LlmConfigTestResult,
  type McpServer,
  type McpServerInput,
  type McpServerPatch,
  type SttConfig,
  type SttConfigInput,
  type SttConfigTestResult,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { sttConfigKeys } from "../../../shared/dictation/index.js";
import { notifyError, notifyInfo, notifySuccess } from "../../../shared/shell/toaster.js";
import {
  CONNECTION_FAILED_TITLE,
  CONNECTION_OK_TITLE,
  LLM_CONNECTION_OK_MESSAGE,
  LLM_REMOVED_MESSAGE,
  LLM_REMOVED_TITLE,
  LLM_SAVED_MESSAGE,
  LLM_SAVED_TITLE,
  SAVE_FAILED_TITLE,
  SERVER_ADD_FAILED_FALLBACK,
  SERVER_ADD_FAILED_TITLE,
  SERVER_ADDED_MESSAGE,
  SERVER_ADDED_TITLE,
  SERVER_REMOVE_FAILED_FALLBACK,
  SERVER_REMOVE_FAILED_TITLE,
  SERVER_REMOVED_TITLE,
  SERVER_UPDATE_FAILED_FALLBACK,
  SERVER_UPDATE_FAILED_TITLE,
  STT_CONNECTION_OK_MESSAGE,
  STT_REMOVED_MESSAGE,
  STT_SAVED_MESSAGE,
} from "../lib/copy.js";
import { saveFailureMessage, serverFailureMessage, testFailureCode } from "../lib/rules.js";
import { assistantSettingsKeys } from "./keys.js";

/** Save the language-model config; success clears the typed key in the caller. */
export function useSaveAssistantLlm() {
  const queryClient = useQueryClient();
  return useMutation<LlmConfig, Error, LlmConfigInput>({
    mutationFn: (input) => putLlmConfig(getAppClient(), input),
    onSuccess: (config) => {
      queryClient.setQueryData(assistantSettingsKeys.llm, config);
      notifySuccess(LLM_SAVED_TITLE, LLM_SAVED_MESSAGE);
    },
    onError: (error) => {
      notifyError(SAVE_FAILED_TITLE, saveFailureMessage(error));
    },
  });
}

/** Probe the saved model endpoint; a failure stamps nothing and reports its code. */
export function useTestAssistantLlm() {
  const queryClient = useQueryClient();
  return useMutation<LlmConfigTestResult, Error, void>({
    mutationFn: () => testLlmConfig(getAppClient()),
    onSuccess: (result) => {
      if (result.ok) {
        notifySuccess(CONNECTION_OK_TITLE, LLM_CONNECTION_OK_MESSAGE);
      } else {
        notifyError(CONNECTION_FAILED_TITLE, testFailureCode(result));
      }
      void queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.llm });
    },
    onError: (error) => {
      notifyError(CONNECTION_FAILED_TITLE, saveFailureMessage(error));
    },
  });
}

/** Remove the model config; the read re-seeds the keyless form. */
export function useRemoveAssistantLlm() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: () => deleteLlmConfig(getAppClient()),
    onSuccess: () => {
      notifyInfo(LLM_REMOVED_TITLE, LLM_REMOVED_MESSAGE);
    },
    onError: (error) => {
      notifyError(SAVE_FAILED_TITLE, saveFailureMessage(error));
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.llm }),
  });
}

/**
 * Re-read the speech config everywhere it is cached: this tab's read
 * and the composers' shared read. A kill-switch refusal calls this so
 * the section narrows and the microphones hide (AGT-064).
 */
function rereadSpeechConfig(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.voice });
  void queryClient.invalidateQueries({ queryKey: sttConfigKeys.all() });
}

/** Save the dictation config; success clears the typed key in the caller. */
export function useSaveAssistantVoice() {
  const queryClient = useQueryClient();
  return useMutation<SttConfig, Error, SttConfigInput>({
    mutationFn: (input) => putSttConfig(getAppClient(), input),
    onSuccess: (config) => {
      queryClient.setQueryData(assistantSettingsKeys.voice, config);
      notifySuccess(LLM_SAVED_TITLE, STT_SAVED_MESSAGE);
    },
    onError: (error) => {
      if (dictationDisabledFrom(error) !== null) rereadSpeechConfig(queryClient);
      notifyError(SAVE_FAILED_TITLE, saveFailureMessage(error));
    },
  });
}

/** Probe the saved dictation endpoint. */
export function useTestAssistantVoice() {
  const queryClient = useQueryClient();
  return useMutation<SttConfigTestResult, Error, void>({
    mutationFn: () => testSttConfig(getAppClient()),
    onSuccess: (result) => {
      if (result.ok) {
        notifySuccess(CONNECTION_OK_TITLE, STT_CONNECTION_OK_MESSAGE);
      } else {
        notifyError(CONNECTION_FAILED_TITLE, testFailureCode(result));
      }
      void queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.voice });
    },
    onError: (error) => {
      if (dictationDisabledFrom(error) !== null) rereadSpeechConfig(queryClient);
      notifyError(CONNECTION_FAILED_TITLE, saveFailureMessage(error));
    },
  });
}

/** Remove the dictation config. */
export function useRemoveAssistantVoice() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: () => deleteSttConfig(getAppClient()),
    onSuccess: () => {
      notifyInfo(LLM_REMOVED_TITLE, STT_REMOVED_MESSAGE);
    },
    onError: (error) => {
      notifyError(SAVE_FAILED_TITLE, saveFailureMessage(error));
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.voice }),
  });
}

/** Register a tool server; answers the created row. */
export function useAddToolServer() {
  const queryClient = useQueryClient();
  return useMutation<McpServer, Error, McpServerInput>({
    mutationFn: (input) => createMcpServer(getAppClient(), input),
    onSuccess: () => {
      notifySuccess(SERVER_ADDED_TITLE, SERVER_ADDED_MESSAGE);
    },
    onError: (error) => {
      notifyError(SERVER_ADD_FAILED_TITLE, serverFailureMessage(error, SERVER_ADD_FAILED_FALLBACK));
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.servers }),
  });
}

export interface ToolServerToggleVars {
  serverId: string;
  isEnabled: boolean;
}

/** Flip one server's enabled flag. */
export function useToggleToolServer() {
  const queryClient = useQueryClient();
  return useMutation<McpServer, Error, ToolServerToggleVars>({
    mutationFn: ({ serverId, isEnabled }) =>
      patchMcpServer(getAppClient(), serverId, { is_enabled: isEnabled } satisfies McpServerPatch),
    onError: (error) => {
      notifyError(SERVER_UPDATE_FAILED_TITLE, serverFailureMessage(error, SERVER_UPDATE_FAILED_FALLBACK));
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.servers }),
  });
}

export interface ToolServerRemoveVars {
  serverId: string;
  name: string;
}

/** Remove one server; its name rides the notice. */
export function useRemoveToolServer() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, ToolServerRemoveVars>({
    mutationFn: ({ serverId }) => deleteMcpServer(getAppClient(), serverId),
    onSuccess: (_result, { name }) => {
      notifyInfo(SERVER_REMOVED_TITLE, `${name} removed.`);
    },
    onError: (error) => {
      notifyError(SERVER_REMOVE_FAILED_TITLE, serverFailureMessage(error, SERVER_REMOVE_FAILED_FALLBACK));
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: assistantSettingsKeys.servers }),
  });
}
