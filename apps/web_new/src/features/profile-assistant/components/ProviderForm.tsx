// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The language-model form (parity/specs/profile.md §10, PROF-021). The
// provider selector switches between a URL-kind endpoint and a named
// provider that carries its own host. The key holds a list of typed model
// IDs (NEWPIDASH-394, design section 7.2) with add and remove, a per-ID
// status, a per-ID Test button, and the provider's message on failure;
// "Save anyway" persists IDs the smoke test could not verify. Removing
// the last ID asks for confirmation, then deletes the key (design
// section 7.3). The key field never shows a stored secret — only a
// placeholder that says a key exists. Save waits for a model plus a
// credential: a typed key, or a stored one where a blank field simply
// keeps it, so a save can never create a config without a secret.

import type { LlmConfig } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import { ModelIdEditor } from "../../assistant/index.js";
import {
  API_KEY_LABEL,
  BASE_URL_LABEL,
  DEFAULT_PROVIDER_KIND,
  LLM_BASE_URL_PLACEHOLDER,
  LLM_KEY_PLACEHOLDER_EMPTY,
  LLM_SECTION_DESCRIPTION,
  LLM_SECTION_HEADING,
  PROVIDER_CHOICES,
  PROVIDER_LABEL,
  REMOVE_LABEL,
  SAVE_LABEL,
} from "../lib/copy.js";
import { endpointVisible, keyPlaceholder, llmCanSave, providerOf } from "../lib/rules.js";
import { ActionRow, SectionFrame } from "./AssistantSections.js";
import { ASSISTANT_FIELD_CLASSES, TextField } from "./TextField.js";

export interface ProviderFormValues {
  provider: string;
  baseUrl: string;
  /** The key's typed model IDs (NEWPIDASH-394, design section 7.2); a key needs at least one. */
  models: string[];
  key: string;
}

export function emptyProviderForm(): ProviderFormValues {
  return { provider: DEFAULT_PROVIDER_KIND, baseUrl: "", models: [], key: "" };
}

/** Seed the form from a stored config; the key always starts blank. */
export function providerFormFrom(config: LlmConfig | undefined): ProviderFormValues {
  if (config === undefined) return emptyProviderForm();
  return {
    provider: providerOf(config),
    baseUrl: config.base_url,
    models: config.models ?? (config.model_name === "" ? [] : [config.model_name]),
    key: "",
  };
}

export interface ProviderFormProps {
  config: LlmConfig | undefined;
  values: ProviderFormValues;
  /** The provider message by model ID from the last refused save or test. */
  failures: Record<string, string>;
  /** Refused/tested IDs the provider rejected (`status: "failed"`); the rest of `failures` is unverified. */
  failedIds?: readonly string[] | undefined;
  /** IDs with a probe in flight; their rows show progress. */
  pendingIds: readonly string[];
  /** Offered while the last refused save left only unverified IDs. */
  saveAnyway: boolean;
  onChange: (values: ProviderFormValues) => void;
  onSave: (saveUnverified: boolean) => void;
  onTestModel: (model: string) => void;
  onRemoveModel: (model: string) => void;
  onRemoveKey: () => void;
  saving: boolean;
  removing: boolean;
}

export function ProviderForm({
  config,
  values,
  failures,
  failedIds,
  pendingIds,
  saveAnyway,
  onChange,
  onSave,
  onTestModel,
  onRemoveModel,
  onRemoveKey,
  saving,
  removing,
}: ProviderFormProps): React.ReactElement {
  const idPrefix = "assistant-llm";
  const showEndpoint = endpointVisible(values.provider);
  const hasKey = config?.has_api_key === true;

  const edit = (patch: Partial<ProviderFormValues>): void => onChange({ ...values, ...patch });

  const providerLabel = PROVIDER_CHOICES.find((choice) => choice.value === values.provider)?.label ?? values.provider;

  return (
    <SectionFrame heading={LLM_SECTION_HEADING} description={LLM_SECTION_DESCRIPTION} testId="assistant-llm">
      <div className="flex flex-col gap-(--space-4)">
        <div className="flex flex-col gap-(--space-2)">
          <label htmlFor={`${idPrefix}-provider`} className="text-body font-medium text-(--text)">
            {PROVIDER_LABEL}
          </label>
          <select
            id={`${idPrefix}-provider`}
            value={values.provider}
            className={ASSISTANT_FIELD_CLASSES}
            onChange={(event) => edit({ provider: event.target.value })}
          >
            {PROVIDER_CHOICES.map((choice) => (
              <option key={choice.value} value={choice.value}>
                {choice.label}
              </option>
            ))}
          </select>
        </div>

        {showEndpoint ? (
          <TextField
            id={`${idPrefix}-base-url`}
            label={BASE_URL_LABEL}
            value={values.baseUrl}
            placeholder={LLM_BASE_URL_PLACEHOLDER}
            onChange={(baseUrl) => edit({ baseUrl })}
          />
        ) : null}

        <ModelIdEditor
          ids={values.models}
          verifiedAt={config?.model_verified_at ?? {}}
          failures={failures}
          failedIds={failedIds}
          pendingIds={pendingIds}
          disabled={saving}
          testable={hasKey}
          confirmLastRemove
          keyLabel={providerLabel}
          onAdd={(model) => edit({ models: [...values.models, model] })}
          onRemove={onRemoveModel}
          onTest={onTestModel}
          onSaveAnyway={saveAnyway ? () => onSave(true) : null}
        />

        <TextField
          id={`${idPrefix}-key`}
          label={API_KEY_LABEL}
          type="password"
          autoComplete="off"
          value={values.key}
          placeholder={keyPlaceholder(hasKey, LLM_KEY_PLACEHOLDER_EMPTY)}
          onChange={(key) => edit({ key })}
        />

        <ActionRow>
          <Button
            variant="primary"
            loading={saving}
            disabled={saving || !llmCanSave({ models: values.models, key: values.key, hasKey })}
            onClick={() => onSave(false)}
          >
            {SAVE_LABEL}
          </Button>
          {hasKey ? (
            <Button variant="danger" loading={removing} onClick={onRemoveKey}>
              {REMOVE_LABEL}
            </Button>
          ) : null}
        </ActionRow>
      </div>
    </SectionFrame>
  );
}
