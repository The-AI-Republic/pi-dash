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
import type { IUserSTTConfig } from "@pi-dash/types";
import { Button } from "@pi-dash/ui";

const service = new AssistantService();

/**
 * Stable anchor for the composer's not-configured mic path to deep-link to
 * (sub-issue #5). When dictation is unconfigured the mic button should route
 * here rather than failing at click time.
 */
export const DICTATION_SETTINGS_ANCHOR = "voice-dictation";

export const DictationSettings = observer(function DictationSettings() {
  const { data: config, mutate } = useSWR<IUserSTTConfig>("assistant-stt-config", () => service.getSTTConfig());

  const [baseUrl, setBaseUrl] = useState("");
  const [modelName, setModelName] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (config) {
      setBaseUrl(config.base_url);
      setModelName(config.model_name);
    }
  }, [config]);

  const save = async () => {
    setSaving(true);
    try {
      await service.putSTTConfig({
        base_url: baseUrl.trim(),
        model_name: modelName.trim(),
        api_key: apiKey || undefined,
      });
      setApiKey("");
      await mutate();
      setToast({ type: TOAST_TYPE.SUCCESS, title: "Saved", message: "Voice dictation configuration updated." });
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
      const res = await service.testSTTConfig();
      if (res.ok) {
        setToast({
          type: TOAST_TYPE.SUCCESS,
          title: "Connection OK",
          message: "Your transcription endpoint responded.",
        });
      } else {
        setToast({ type: TOAST_TYPE.ERROR, title: "Connection failed", message: res.error_code || "Unknown error" });
      }
      await mutate();
    } finally {
      setTesting(false);
    }
  };

  const remove = async () => {
    await service.deleteSTTConfig();
    setApiKey("");
    setBaseUrl("");
    setModelName("");
    await mutate();
    setToast({ type: TOAST_TYPE.INFO, title: "Removed", message: "Voice dictation configuration deleted." });
  };

  return (
    <div id={DICTATION_SETTINGS_ANCHOR} className="flex max-w-xl flex-col gap-5">
      <div>
        <h3 className="text-16 font-semibold text-primary">Voice dictation</h3>
        <p className="mt-1 text-13 text-secondary">
          Hold the mic button in the chat composer to record, release to transcribe. Connect your own OpenAI-compatible
          speech-to-text endpoint — the <code>/v1/audio/transcriptions</code> route — with its base URL, API key, and
          model.
        </p>
        <p className="mt-2 text-13 text-secondary">
          Your audio is sent directly to the endpoint you configure here and is not stored by Pi Dash.
        </p>
      </div>

      <label className="flex flex-col gap-1 text-13">
        <span className="text-secondary">Base URL</span>
        <input
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          placeholder="https://api.openai.com/v1"
          className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
        />
      </label>

      <label className="flex flex-col gap-1 text-13">
        <span className="text-secondary">Model</span>
        <input
          value={modelName}
          onChange={(e) => setModelName(e.target.value)}
          placeholder="whisper-1"
          className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
        />
      </label>

      <label className="flex flex-col gap-1 text-13">
        <span className="text-secondary">API key</span>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          placeholder={config?.has_api_key ? "•••• (saved) — enter to replace" : "Your transcription API key"}
          className="rounded-md border border-subtle bg-surface-1 px-3 py-2"
        />
      </label>

      <div className="flex items-center gap-2">
        <Button onClick={save} loading={saving} disabled={!baseUrl.trim() || !modelName.trim()}>
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
