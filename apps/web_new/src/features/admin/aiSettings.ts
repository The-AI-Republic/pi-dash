// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI settings logic (ADM-046, ADM-047, from
// parity/specs/admin.md §6.1/§6.2): the configuration keys the form
// reads and writes, the oracle-pinned link targets and success
// wording, the pure row selector and patch builder, and the save
// mutation — always enabled, success notice on save, log-only on
// failure (accepted OSS quirk).
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateInstanceConfigurations, type InstanceConfigurationList } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { notifySuccess } from "../../shared/shell/index.js";
import { adminKeys } from "./queries.js";

/** Configuration key holding the instance LLM model name. */
export const LLM_MODEL_KEY = "LLM_MODEL";

/** Configuration key holding the instance LLM API key. */
export const LLM_API_KEY = "LLM_API_KEY";

/** Vendor model catalogue the model helper link points at. */
export const AI_MODEL_DOCS_URL = "https://platform.openai.com/docs/models";

/** Vendor key-management page the key helper link points at. */
export const AI_KEY_DOCS_URL = "https://platform.openai.com/api-keys";

/** Company contact page the other-vendor callout links to. */
export const AI_VENDOR_CONTACT_URL = "https://airepublic.com/contact";

/** Success notice title the parity scenario pins exactly. */
export const AI_SETTINGS_SAVED_TITLE = "Success";

/** Success notice wording the parity scenario pins as a substring. */
export const AI_SETTINGS_SAVED_MESSAGE = "AI Settings updated successfully.";

/** The two AI values the form edits. */
export interface AiSettingsValues {
  model: string;
  apiKey: string;
}

/**
 * The AI values out of the shared configurations rows: missing keys
 * and null values read as empty, so the form always has strings.
 */
export function selectAiSettings(rows: InstanceConfigurationList | undefined): AiSettingsValues {
  const byKey = new Map((rows ?? []).map((row) => [row.key, row.value ?? ""]));
  return {
    model: byKey.get(LLM_MODEL_KEY) ?? "",
    apiKey: byKey.get(LLM_API_KEY) ?? "",
  };
}

/**
 * The save payload: both keys always, including empty strings — an
 * empty save is allowed and clears the stored values.
 */
export function buildAiSettingsPatch(model: string, apiKey: string): Record<string, string> {
  return { [LLM_MODEL_KEY]: model, [LLM_API_KEY]: apiKey };
}

/**
 * Persist the AI values (ADM-046): the write carries both keys, a
 * success notice fires and the shared configurations re-read so saved
 * forms re-seed. A failed save logs to the console only — no notice —
 * preserving the accepted OSS quirk.
 */
export function useSaveAiSettings() {
  const queryClient = useQueryClient();
  return useMutation<void, Error, AiSettingsValues>({
    mutationFn: async (input) => {
      await updateInstanceConfigurations(getAppClient(), buildAiSettingsPatch(input.model, input.apiKey));
    },
    onSuccess: () => {
      notifySuccess(AI_SETTINGS_SAVED_TITLE, AI_SETTINGS_SAVED_MESSAGE);
      void queryClient.invalidateQueries({ queryKey: adminKeys.configurations });
    },
    onError: (error) => {
      console.error(error);
    },
  });
}
