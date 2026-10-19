// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The voice-dictation form (parity/specs/profile.md §10, PROF-022). It
// mirrors the model form without a provider selector: the endpoint, the
// model and the key are all required, a stored key only ever shows its
// placeholder, and testing waits for a saved key. When the instance kill
// switch is off the section narrows: no form at all, and the remove
// control only while a key is still stored.

import type { SttConfig } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import {
  API_KEY_LABEL,
  BASE_URL_LABEL,
  MODEL_LABEL,
  REMOVE_LABEL,
  SAVE_LABEL,
  STT_BASE_URL_PLACEHOLDER,
  STT_KEY_PLACEHOLDER_EMPTY,
  STT_MODEL_PLACEHOLDER,
  TEST_LABEL,
  VOICE_SECTION_DESCRIPTION,
  VOICE_SECTION_HEADING,
  VOICE_UNAVAILABLE_NOTE,
} from "../lib/copy.js";
import { canTest, keyPlaceholder, lastVerifiedLine, sttCanSave, voiceSection } from "../lib/rules.js";
import { ActionRow, SectionFrame } from "./AssistantSections.js";
import { TextField } from "./TextField.js";

export interface VoiceFormValues {
  baseUrl: string;
  model: string;
  key: string;
}

export function emptyVoiceForm(): VoiceFormValues {
  return { baseUrl: "", model: "", key: "" };
}

/** Seed the form from a stored config; the key always starts blank. */
export function voiceFormFrom(config: SttConfig | undefined): VoiceFormValues {
  if (config === undefined) return emptyVoiceForm();
  return { baseUrl: config.base_url, model: config.model_name, key: "" };
}

export interface VoiceFormProps {
  config: SttConfig | undefined;
  values: VoiceFormValues;
  onChange: (values: VoiceFormValues) => void;
  onSave: () => void;
  onTest: () => void;
  onRemove: () => void;
  saving: boolean;
  testing: boolean;
  removing: boolean;
}

export function VoiceForm({
  config,
  values,
  onChange,
  onSave,
  onTest,
  onRemove,
  saving,
  testing,
  removing,
}: VoiceFormProps): React.ReactElement | null {
  const shape = voiceSection(config);
  const idPrefix = "assistant-voice";
  const hasKey = config?.has_api_key === true;

  if (shape.kind === "hidden") return null;

  if (shape.kind === "removal-only") {
    return (
      <SectionFrame heading={VOICE_SECTION_HEADING} description={VOICE_UNAVAILABLE_NOTE} testId="assistant-voice">
        <ActionRow>
          <Button variant="danger" loading={removing} onClick={onRemove}>
            {REMOVE_LABEL}
          </Button>
        </ActionRow>
      </SectionFrame>
    );
  }

  const verified = lastVerifiedLine(config?.last_verified_at);
  const edit = (patch: Partial<VoiceFormValues>): void => onChange({ ...values, ...patch });

  return (
    <SectionFrame heading={VOICE_SECTION_HEADING} description={VOICE_SECTION_DESCRIPTION} testId="assistant-voice">
      <div className="flex flex-col gap-(--space-4)">
        <TextField
          id={`${idPrefix}-base-url`}
          label={BASE_URL_LABEL}
          value={values.baseUrl}
          placeholder={STT_BASE_URL_PLACEHOLDER}
          onChange={(baseUrl) => edit({ baseUrl })}
        />
        <TextField
          id={`${idPrefix}-model`}
          label={MODEL_LABEL}
          value={values.model}
          placeholder={STT_MODEL_PLACEHOLDER}
          onChange={(model) => edit({ model })}
        />
        <TextField
          id={`${idPrefix}-key`}
          label={API_KEY_LABEL}
          type="password"
          autoComplete="off"
          value={values.key}
          placeholder={keyPlaceholder(hasKey, STT_KEY_PLACEHOLDER_EMPTY)}
          onChange={(key) => edit({ key })}
        />

        <ActionRow>
          <Button variant="primary" loading={saving} disabled={saving || !sttCanSave(values)} onClick={onSave}>
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
