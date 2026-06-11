/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import { AssistantService } from "@pi-dash/services";
import type { IUserLLMConfig, TAssistantProviderKind } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";

const service = new AssistantService();

const KNOWN_MODELS = [
  "anthropic/claude-sonnet-4-6",
  "meta-llama/llama-3.3-70b-instruct",
  "qwen/qwen-2.5-72b-instruct",
  "deepseek/deepseek-chat",
];

export const AIAssistantProfileSettings = observer(function AIAssistantProfileSettings() {
  const { data: config, mutate } = useSWR<IUserLLMConfig>("assistant-llm-config", () => service.getLLMConfig());

  const [provider, setProvider] = useState<TAssistantProviderKind>("openai_compatible");
  const [baseUrl, setBaseUrl] = useState("");
  const [modelName, setModelName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (config) {
      setProvider(config.provider_kind);
      setBaseUrl(config.base_url);
      setModelName(config.model_name);
    }
  }, [config]);

  const save = async () => {
    setSaving(true);
    try {
      await service.putLLMConfig({
        provider_kind: provider,
        base_url: provider === "anthropic" ? undefined : baseUrl,
        model_name: modelName,
        api_key: apiKey || undefined,
      });
      setApiKey("");
      await mutate();
      setToast({ type: TOAST_TYPE.SUCCESS, title: "Saved", message: "AI provider configuration updated." });
    } catch (e: unknown) {
      const err = e as { detail?: string; error?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Save failed",
        message: err?.detail || err?.error || "Invalid configuration",
      });
    } finally {
      setSaving(false);
    }
  };

  const test = async () => {
    setTesting(true);
    try {
      const res = await service.testLLMConfig();
      if (res.ok) {
        setToast({ type: TOAST_TYPE.SUCCESS, title: "Connection OK", message: "Your provider responded." });
      } else {
        setToast({ type: TOAST_TYPE.ERROR, title: "Connection failed", message: res.error_code || "Unknown error" });
      }
      await mutate();
    } finally {
      setTesting(false);
    }
  };

  const remove = async () => {
    await service.deleteLLMConfig();
    setApiKey("");
    setBaseUrl("");
    setModelName("");
    await mutate();
    setToast({ type: TOAST_TYPE.INFO, title: "Removed", message: "AI provider configuration deleted." });
  };

  return (
    <div className="flex max-w-xl flex-col gap-5">
      <div>
        <h3 className="text-16 font-semibold text-primary">AI Assistant</h3>
        <p className="mt-1 text-13 text-secondary">
          Connect your own LLM provider to use the assistant. Tool-calling quality varies by model — we recommend models
          with native function-calling support.
        </p>
      </div>

      <label className="flex flex-col gap-1 text-13">
        <span className="text-secondary">Provider</span>
        <select
          value={provider}
          onChange={(e) => setProvider(e.target.value as TAssistantProviderKind)}
          className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
        >
          <option value="openai_compatible">OpenAI-compatible (OpenRouter, vLLM, …)</option>
          <option value="anthropic">Anthropic</option>
        </select>
      </label>

      {provider === "openai_compatible" && (
        <label className="flex flex-col gap-1 text-13">
          <span className="text-secondary">Base URL</span>
          <input
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://openrouter.ai/api/v1"
            className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
          />
        </label>
      )}

      <label className="flex flex-col gap-1 text-13">
        <span className="text-secondary">Model</span>
        <input
          value={modelName}
          onChange={(e) => setModelName(e.target.value)}
          list="assistant-known-models"
          placeholder="meta-llama/llama-3.3-70b-instruct"
          className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
        />
        <datalist id="assistant-known-models">
          {KNOWN_MODELS.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
      </label>

      <label className="flex flex-col gap-1 text-13">
        <span className="text-secondary">API key</span>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          placeholder={config?.has_api_key ? "•••• (saved) — enter to replace" : "Your provider API key"}
          className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
        />
      </label>

      <div className="flex items-center gap-2">
        <Button onClick={save} loading={saving} disabled={!modelName.trim()}>
          Save
        </Button>
        <Button onClick={test} variant="neutral-primary" loading={testing} disabled={!config?.has_api_key}>
          Test connection
        </Button>
        {config?.has_api_key && (
          <Button onClick={remove} variant="tertiary-danger">
            Remove
          </Button>
        )}
      </div>
      {config?.last_verified_at && (
        <div className="text-12 text-secondary">
          Last verified: {new Date(config.last_verified_at).toLocaleString()}
        </div>
      )}
    </div>
  );
});
