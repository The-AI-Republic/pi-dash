// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The language-model form (parity/specs/profile.md §10, PROF-021). The
// provider selector switches between a URL-kind endpoint and a named
// provider that carries its own host, the model accepts any name, and the
// key field never shows a stored secret — only a placeholder that says a
// key exists. Save waits for a typed key, so a form can never overwrite a
// stored credential with nothing.

import type { LlmConfig } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import {
  API_KEY_LABEL,
  BASE_URL_LABEL,
  DEFAULT_PROVIDER_KIND,
  LLM_BASE_URL_PLACEHOLDER,
  LLM_KEY_PLACEHOLDER_EMPTY,
  LLM_MODEL_PLACEHOLDER,
  LLM_SECTION_DESCRIPTION,
  LLM_SECTION_HEADING,
  MODEL_LABEL,
  PROVIDER_CHOICES,
  PROVIDER_LABEL,
  REMOVE_LABEL,
  SAVE_LABEL,
  TEST_LABEL,
} from "../lib/copy.js";
import { canTest, endpointVisible, keyPlaceholder, lastVerifiedLine, llmCanSave, providerOf } from "../lib/rules.js";
import { ActionRow, SectionFrame } from "./AssistantSections.js";
import { ASSISTANT_FIELD_CLASSES, TextField } from "./TextField.js";

export interface ProviderFormValues {
  provider: string;
  baseUrl: string;
  model: string;
  key: string;
}

export function emptyProviderForm(): ProviderFormValues {
  return { provider: DEFAULT_PROVIDER_KIND, baseUrl: "", model: "", key: "" };
}

/** Seed the form from a stored config; the key always starts blank. */
export function providerFormFrom(config: LlmConfig | undefined): ProviderFormValues {
  if (config === undefined) return emptyProviderForm();
  return {
    provider: providerOf(config),
    baseUrl: config.base_url,
    model: config.model_name,
    key: "",
  };
}

export interface ProviderFormProps {
  config: LlmConfig | undefined;
  values: ProviderFormValues;
  onChange: (values: ProviderFormValues) => void;
  onSave: () => void;
  onTest: () => void;
  onRemove: () => void;
  saving: boolean;
  testing: boolean;
  removing: boolean;
}

export function ProviderForm({
  config,
  values,
  onChange,
  onSave,
  onTest,
  onRemove,
  saving,
  testing,
  removing,
}: ProviderFormProps): React.ReactElement {
  const idPrefix = "assistant-llm";
  const verified = lastVerifiedLine(config?.last_verified_at);
  const showEndpoint = endpointVisible(values.provider);
  const hasKey = config?.has_api_key === true;

  const edit = (patch: Partial<ProviderFormValues>): void => onChange({ ...values, ...patch });

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

        <TextField
          id={`${idPrefix}-model`}
          label={MODEL_LABEL}
          value={values.model}
          placeholder={LLM_MODEL_PLACEHOLDER}
          onChange={(model) => edit({ model })}
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
          <Button variant="primary" loading={saving} disabled={saving || !llmCanSave(values)} onClick={onSave}>
            {SAVE_LABEL}
          </Button>
          <Button loading={testing} disabled={testing || !canTest(config)} onClick={onTest}>
            {TEST_LABEL}
          </Button>
          {hasKey ? (
            <Button variant="danger" loading={removing} onClick={onRemove}>
              {REMOVE_LABEL}
            </Button>
          ) : null}
        </ActionRow>

        {verified === null ? null : <p className="text-meta text-(--text-muted)">{verified}</p>}
      </div>
    </SectionFrame>
  );
}
