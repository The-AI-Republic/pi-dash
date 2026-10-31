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

import * as React from "react";

import {
  useRemoveAssistantLlm,
  useRemoveAssistantVoice,
  useSaveAssistantLlm,
  useSaveAssistantVoice,
  useTestAssistantLlm,
  useTestAssistantVoice,
} from "../api/mutations.js";
import { useAssistantLlm, useAssistantVoice } from "../api/queries.js";
import { endpointForSave, keyForSave } from "../lib/rules.js";
import { AssistantSectionsSkeleton } from "./AssistantSectionsSkeleton.js";
import { emptyProviderForm, providerFormFrom, ProviderForm, type ProviderFormValues } from "./ProviderForm.js";
import { ToolServersSection } from "./ToolServersSection.js";
import { emptyVoiceForm, voiceFormFrom, VoiceForm, type VoiceFormValues } from "./VoiceForm.js";

export function AssistantSettingsPage(): React.ReactElement {
  const llm = useAssistantLlm();
  const voice = useAssistantVoice();

  const saveLlm = useSaveAssistantLlm();
  const testLlm = useTestAssistantLlm();
  const removeLlm = useRemoveAssistantLlm();
  const saveVoice = useSaveAssistantVoice();
  const testVoice = useTestAssistantVoice();
  const removeVoice = useRemoveAssistantVoice();

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

  const saveProvider = (): void => {
    const baseUrl = endpointForSave(providerForm.provider, providerForm.baseUrl);
    const apiKey = keyForSave(providerForm.key);
    saveLlm.mutate(
      {
        provider_kind: providerForm.provider,
        ...(baseUrl === undefined ? {} : { base_url: baseUrl }),
        model_name: providerForm.model.trim(),
        ...(apiKey === undefined ? {} : { api_key: apiKey }),
      },
      {
        onSuccess: () => setProviderForm((previous) => ({ ...previous, key: "" })),
      }
    );
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
      <ProviderForm
        config={llm.data}
        values={providerForm}
        onChange={setProviderForm}
        onSave={saveProvider}
        onTest={() => testLlm.mutate()}
        onRemove={() => removeLlm.mutate()}
        saving={saveLlm.isPending}
        testing={testLlm.isPending}
        removing={removeLlm.isPending}
      />

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

      <ToolServersSection />
    </div>
  );
}
