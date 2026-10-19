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
  useAddToolServer,
  useRemoveAssistantLlm,
  useRemoveAssistantVoice,
  useRemoveToolServer,
  useSaveAssistantLlm,
  useSaveAssistantVoice,
  useTestAssistantLlm,
  useTestAssistantVoice,
  useToggleToolServer,
} from "../api/mutations.js";
import { useAssistantLlm, useAssistantServers, useAssistantVoice } from "../api/queries.js";
import { endpointForSave } from "../lib/rules.js";
import { AssistantSectionsSkeleton } from "./AssistantSectionsSkeleton.js";
import { emptyProviderForm, providerFormFrom, ProviderForm, type ProviderFormValues } from "./ProviderForm.js";
import { emptyToolServerDraft, ToolServersForm, type ToolServerDraft } from "./ToolServersForm.js";
import { emptyVoiceForm, voiceFormFrom, VoiceForm, type VoiceFormValues } from "./VoiceForm.js";

export function AssistantSettingsPage(): React.ReactElement {
  const llm = useAssistantLlm();
  const voice = useAssistantVoice();
  const servers = useAssistantServers();

  const saveLlm = useSaveAssistantLlm();
  const testLlm = useTestAssistantLlm();
  const removeLlm = useRemoveAssistantLlm();
  const saveVoice = useSaveAssistantVoice();
  const testVoice = useTestAssistantVoice();
  const removeVoice = useRemoveAssistantVoice();
  const addServer = useAddToolServer();
  const toggleServer = useToggleToolServer();
  const removeServer = useRemoveToolServer();

  const [providerForm, setProviderForm] = React.useState<ProviderFormValues>(emptyProviderForm);
  const [voiceForm, setVoiceForm] = React.useState<VoiceFormValues>(emptyVoiceForm);
  const [serverDraft, setServerDraft] = React.useState<ToolServerDraft>(emptyToolServerDraft);
  const [pendingServerId, setPendingServerId] = React.useState<string | null>(null);

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
    saveLlm.mutate(
      {
        provider_kind: providerForm.provider,
        ...(baseUrl === undefined ? {} : { base_url: baseUrl }),
        model_name: providerForm.model.trim(),
        api_key: providerForm.key.trim(),
      },
      {
        onSuccess: () => setProviderForm((previous) => ({ ...previous, key: "" })),
      }
    );
  };

  const saveVoiceConfig = (): void => {
    saveVoice.mutate(
      {
        base_url: voiceForm.baseUrl.trim(),
        model_name: voiceForm.model.trim(),
        api_key: voiceForm.key.trim(),
      },
      {
        onSuccess: () => setVoiceForm((previous) => ({ ...previous, key: "" })),
      }
    );
  };

  const addToolServer = (): void => {
    const auth = serverDraft.authHeader.trim();
    addServer.mutate(
      {
        name: serverDraft.name.trim(),
        url: serverDraft.url.trim(),
        ...(auth === "" ? {} : { auth_header: auth }),
      },
      { onSuccess: () => setServerDraft(emptyToolServerDraft()) }
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

      <ToolServersForm
        servers={servers.data}
        draft={serverDraft}
        onDraftChange={setServerDraft}
        onAdd={addToolServer}
        onToggle={(server) => {
          setPendingServerId(server.id);
          toggleServer.mutate(
            { serverId: server.id, isEnabled: !server.is_enabled },
            { onSettled: () => setPendingServerId(null) }
          );
        }}
        onRemove={(server) => {
          setPendingServerId(server.id);
          removeServer.mutate(
            { serverId: server.id, name: server.name },
            { onSettled: () => setPendingServerId(null) }
          );
        }}
        adding={addServer.isPending}
        pendingId={pendingServerId}
      />
    </div>
  );
}
