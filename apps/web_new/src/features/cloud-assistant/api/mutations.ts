// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud assistant writes (parity/specs/cloud.md §7, CLOUD-032/033). Each
// action refreshes the connection list after it settles, because the
// server owns which row is active, which row is connected, and which
// managed models the account exposes. Notices are raised by the components
// that own the flow so a button that unmounts mid-request can never be the
// only place its outcome is reported.

import {
  createAssistantConnection,
  deleteAssistantConnection,
  disconnectAssistantConnection,
  getOpenAILoginStatus,
  isApiError,
  selectAssistantConnection,
  startOpenAILogin,
  testAssistantConnectionModel,
  testLlmConfig,
  updateConnectionModel,
  updateConnectionModels,
  type AssistantConnection,
  type AssistantConnectionCreateInput,
  type ApiClient,
  type LlmConfigTestResult,
  type ModelTestResult,
  type OpenAILoginStatus,
  type OpenAILoginStart,
} from "@pidash/api-client";
import { useQueryClient } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { cloudAssistantKeys } from "./keys.js";

export interface CloudAssistantActions {
  create(input: AssistantConnectionCreateInput): Promise<AssistantConnection>;
  select(id: string): Promise<AssistantConnection>;
  setModel(id: string, model: string): Promise<AssistantConnection>;
  /**
   * Replace a connection's model-ID list (NEWPIDASH-394, design section
   * 6.3). A refused write (`400 model_test_failed`, nothing saved)
   * rejects; read its per-ID results with `modelTestResultsFrom`.
   */
  setModels(id: string, models: string[], saveUnverified?: boolean): Promise<AssistantConnection>;
  /** Smoke-test one stored model ID (design section 6.4); throttled server-side. */
  testModel(id: string, model: string): Promise<ModelTestResult>;
  remove(id: string): Promise<void>;
  disconnect(id: string): Promise<void>;
  test(): Promise<LlmConfigTestResult>;
  startSignIn(): Promise<OpenAILoginStart>;
  signInStatus(state: string): Promise<OpenAILoginStatus>;
}

/** The client-bound actions, so tests can drive them without React. */
export function cloudAssistantActions(client: ApiClient): CloudAssistantActions {
  return {
    create: (input) => createAssistantConnection(client, input),
    select: (id) => selectAssistantConnection(client, id),
    setModel: (id, model) => updateConnectionModel(client, id, model),
    setModels: (id, models, saveUnverified) =>
      updateConnectionModels(client, id, { models, ...(saveUnverified === undefined ? {} : { saveUnverified }) }),
    testModel: (id, model) => testAssistantConnectionModel(client, id, model),
    remove: (id) => deleteAssistantConnection(client, id),
    disconnect: (id) => disconnectAssistantConnection(client, id),
    test: () => testLlmConfig(client),
    startSignIn: () => startOpenAILogin(client),
    signInStatus: (state) => getOpenAILoginStatus(client, state),
  };
}

/**
 * One write, one refresh. The list is the single source of selection and
 * connection state, so every action invalidates it rather than guessing
 * the new shape; the config read is invalidated too because a lane change
 * re-gates the assistant entry points.
 */
export function useCloudAssistantActions(): CloudAssistantActions & { invalidate(): Promise<void> } {
  const queryClient = useQueryClient();
  return {
    ...cloudAssistantActions(getAppClient()),
    invalidate: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: cloudAssistantKeys.connections() }),
        queryClient.invalidateQueries({ queryKey: cloudAssistantKeys.config() }),
      ]);
    },
  };
}

/** The server's own detail for a failed write, when it sent one. */
export function actionFailureDetail(error: unknown, fallback: string): string {
  if (isApiError(error)) {
    if (error.message !== "") return error.message;
    const first = Object.values(error.fields ?? {})[0];
    if (first !== undefined) {
      const detail = first[0];
      if (detail !== undefined && detail !== "") return detail;
    }
  }
  return fallback;
}
