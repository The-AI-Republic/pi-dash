// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI-assistant settings tab (parity/specs/profile.md §10, PROF-021..023).
// Three sections stack in one column: the language model, voice dictation
// and the tool-server registry. Each form seeds once from its server read
// and then belongs to the member; a saved key is never echoed into the
// field, so the placeholder is the only sign it exists. Nothing here
// refetches on idle, listens for shortcuts or accepts a drop (PROF-027),
// and every write targets the signed-in member's own record (PROF-028).

import { modelTestResultsFrom } from "@pidash/api-client";
import { useParams } from "@tanstack/react-router";
import * as React from "react";

import { modelFailureDetails, saveAnywayModels } from "../../assistant/index.js";
import {
  useRemoveAssistantLlm,
  useRemoveAssistantVoice,
  useSaveAssistantLlm,
  useSaveAssistantVoice,
  useTestAssistantLlmModel,
  useTestAssistantVoice,
} from "../api/mutations.js";
import { useAssistantLlm, useAssistantVoice } from "../api/queries.js";
import { endpointForSave, keyForSave } from "../lib/rules.js";
import { DICTATION_UI_ENABLED } from "../../../shared/dictation/index.js";
import { AssistantModelsSection } from "./AssistantModelsSection.js";
import { AssistantSectionsSkeleton } from "./AssistantSectionsSkeleton.js";
import { emptyProviderForm, providerFormFrom, ProviderForm, type ProviderFormValues } from "./ProviderForm.js";
import { ToolServersSection } from "./ToolServersSection.js";
import { emptyVoiceForm, voiceFormFrom, VoiceForm, type VoiceFormValues } from "./VoiceForm.js";

export function AssistantSettingsPage(): React.ReactElement {
  const { ws } = useParams({ strict: false }) as { ws?: string };
  const llm = useAssistantLlm();
  const voice = useAssistantVoice();

  const saveLlm = useSaveAssistantLlm();
  const testLlmModel = useTestAssistantLlmModel();
  const removeLlm = useRemoveAssistantLlm();
  const saveVoice = useSaveAssistantVoice();
  const testVoice = useTestAssistantVoice();
  const removeVoice = useRemoveAssistantVoice();

  // Per-ID write/test state for the key's model editor (NEWPIDASH-394,
  // design sections 7.2 and 7.3): the provider message by model from the
  // last refused save or test, the IDs with a probe in flight, and
  // whether a "Save anyway" retry is on offer.
  const [llmFailures, setLlmFailures] = React.useState<Record<string, string>>({});
  const [llmPendingIds, setLlmPendingIds] = React.useState<readonly string[]>([]);
  const [llmSaveAnyway, setLlmSaveAnyway] = React.useState(false);

  const [providerForm, setProviderForm] = React.useState<ProviderFormValues>(emptyProviderForm);
  const [voiceForm, setVoiceForm] = React.useState<VoiceFormValues>(emptyVoiceForm);

  // A config read seeds its form once; a later read only re-seeds when it
  // carries different values, so a write's answer never wipes typing.
  const seededLlm = React.useRef<string | null>(null);
  const seedKey = React.useMemo(() => JSON.stringify(llm.data ?? null), [llm.data]);
  React.useEffect(() => {
    if (llm.data === undefined) return;
    if (seededLlm.current === seedKey) return;
    seededLlm.current = seedKey;
    setProviderForm(providerFormFrom(llm.data));
  }, [llm.data, seedKey]);

  const seededVoice = React.useRef<string | null>(null);
  const voiceSeedKey = React.useMemo(() => JSON.stringify(voice.data ?? null), [voice.data]);
  React.useEffect(() => {
    if (voice.data === undefined) return;
    if (seededVoice.current === voiceSeedKey) return;
    seededVoice.current = voiceSeedKey;
    setVoiceForm(voiceFormFrom(voice.data));
  }, [voice.data, voiceSeedKey]);

  if (llm.data === undefined || voice.data === undefined) {
    return <AssistantSectionsSkeleton />;
  }

  const saveProvider = (saveUnverified: boolean): void => {
    const baseUrl = endpointForSave(providerForm.provider, providerForm.baseUrl);
    const apiKey = keyForSave(providerForm.key);
    const models = providerForm.models;
    saveLlm.mutate(
      {
        provider_kind: providerForm.provider,
        ...(baseUrl === undefined ? {} : { base_url: baseUrl }),
        model_name: models[0] ?? "",
        models,
        ...(saveUnverified ? { save_unverified: true } : {}),
        ...(apiKey === undefined ? {} : { api_key: apiKey }),
      },
      {
        onSuccess: (config) => {
          setProviderForm((previous) => ({ ...previous, key: "", models: config.models ?? previous.models }));
          setLlmFailures({});
          setLlmSaveAnyway(false);
        },
        onError: (error) => {
          // The hook stays silent on refused writes; the provider's
          // message lands per ID in the form instead.
          const results = modelTestResultsFrom(error);
          if (results === null) return;
          setLlmFailures(modelFailureDetails(results));
          setLlmSaveAnyway(saveAnywayModels(results) !== null);
        },
      }
    );
  };

  const testProviderModel = (model: string): void => {
    setLlmPendingIds([model]);
    testLlmModel.mutate(model, {
      onSuccess: (result) => {
        if (result.status === "verified") {
          setLlmFailures((current) => {
            if (current[model] === undefined) return current;
            const next = { ...current };
            delete next[model];
            return next;
          });
        } else {
          setLlmFailures((current) => ({ ...current, [model]: result.detail ?? result.code ?? "" }));
        }
      },
      onSettled: () => setLlmPendingIds([]),
    });
  };

  const removeProviderModel = (model: string): void => {
    // The editor confirms the last ID; removing it deletes the key.
    if (providerForm.models.length <= 1) {
      removeLlm.mutate();
      return;
    }
    setProviderForm((previous) => ({ ...previous, models: previous.models.filter((kept) => kept !== model) }));
    setLlmFailures((current) => {
      if (current[model] === undefined) return current;
      const next = { ...current };
      delete next[model];
      return next;
    });
  };

  const saveVoiceConfig = (): void => {
    const apiKey = keyForSave(voiceForm.key);
    saveVoice.mutate(
      {
        base_url: voiceForm.baseUrl.trim(),
        model_name: voiceForm.model.trim(),
        ...(apiKey === undefined ? {} : { api_key: apiKey }),
      },
      {
        onSuccess: () => setVoiceForm((previous) => ({ ...previous, key: "" })),
      }
    );
  };

  return (
    <div data-testid="assistant-settings-page" className="flex max-w-2xl flex-col gap-(--space-8)">
      {ws === undefined ? null : <AssistantModelsSection workspaceSlug={ws} config={llm.data} />}
      <ProviderForm
        config={llm.data}
        values={providerForm}
        failures={llmFailures}
        pendingIds={llmPendingIds}
        saveAnyway={llmSaveAnyway}
        onChange={setProviderForm}
        onSave={(saveUnverified) => saveProvider(saveUnverified)}
        onTestModel={(model) => testProviderModel(model)}
        onRemoveModel={(model) => removeProviderModel(model)}
        onRemoveKey={() => removeLlm.mutate()}
        saving={saveLlm.isPending}
        removing={removeLlm.isPending}
      />

      {/* Dictation is hidden until it ships (DICTATION_UI_ENABLED). */}
      {DICTATION_UI_ENABLED ? (
        <VoiceForm
          config={voice.data}
          values={voiceForm}
          onChange={setVoiceForm}
          onSave={saveVoiceConfig}
          onTest={() => testVoice.mutate()}
          onRemove={() => removeVoice.mutate()}
          saving={saveVoice.isPending}
          testing={testVoice.isPending}
          removing={removeVoice.isPending}
        />
      ) : null}

      <ToolServersSection />
    </div>
  );
}
