// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add a connection (parity/specs/cloud.md §7, CLOUD-033). The key form asks
// for a provider, a base address for the compatible kind, a model and a
// secret; save stays disabled until all four exist. The server is the
// authority on reachability, so its refusal stays in the form under the
// fields instead of clearing them. Provider sign-in lives on the row that
// needs it, not here.

import { Button, FieldScaffold, Input } from "@pidash/kit";
import * as React from "react";

import {
  ADD_ACTION,
  ADD_FAILED_FALLBACK,
  ADD_FAILED_TITLE,
  ADD_SAVING_ACTION,
  ANTHROPIC_PROVIDER_LABEL,
  API_KEY_LABEL,
  API_KEY_PLACEHOLDER,
  BASE_URL_LABEL,
  BASE_URL_PLACEHOLDER,
  COMPATIBLE_PROVIDER_LABEL,
  MODEL_LABEL,
  MODEL_PLACEHOLDER,
  PROVIDER_LABEL,
} from "../lib/copy.js";
import {
  BYOK_PROVIDERS,
  EMPTY_BYOK_DRAFT,
  needsBaseUrl,
  validateByokDraft,
  type ByokDraft,
  type ByokProvider,
} from "../lib/byok.js";
import { BYOK_MODEL_SUGGESTIONS } from "../lib/rows.js";

export interface AddConnectionViewProps {
  saving: boolean;
  error: string | null;
  onSave(draft: ByokDraft): void;
}

export function AddConnectionView({ saving, error, onSave }: AddConnectionViewProps): React.ReactElement {
  const [draft, setDraft] = React.useState<ByokDraft>(EMPTY_BYOK_DRAFT);
  const problems = validateByokDraft(draft);
  const baseVisible = needsBaseUrl(draft.provider);
  const enabled = !saving && Object.values(problems).every((problem) => problem === null);

  const update = (patch: Partial<ByokDraft>): void => {
    setDraft((current) => ({ ...current, ...patch }));
  };

  return (
    <form
      aria-label="Add connection"
      noValidate
      className="flex max-w-xl flex-col gap-(--space-4)"
      onSubmit={(event) => {
        event.preventDefault();
        if (!enabled) return;
        onSave(draft);
      }}
    >
      <label className="text-body flex flex-col gap-(--space-1)">
        <span className="text-(--text-muted)">{PROVIDER_LABEL}</span>
        <select
          aria-label={PROVIDER_LABEL}
          value={draft.provider}
          onChange={(event) => update({ provider: event.target.value as ByokProvider })}
          className="text-body h-(--control-height) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
        >
          {BYOK_PROVIDERS.map((provider) => (
            <option key={provider} value={provider}>
              {provider === "anthropic" ? ANTHROPIC_PROVIDER_LABEL : COMPATIBLE_PROVIDER_LABEL}
            </option>
          ))}
        </select>
      </label>

      {baseVisible ? (
        <Input
          label={BASE_URL_LABEL}
          placeholder={BASE_URL_PLACEHOLDER}
          value={draft.baseUrl}
          onChange={(event) => update({ baseUrl: event.target.value })}
        />
      ) : null}

      <FieldScaffold label={MODEL_LABEL} fieldId="cloud-assistant-model">
        {(describedBy) => (
          <>
            <input
              id="cloud-assistant-model"
              aria-describedby={describedBy}
              list="assistant-known-byok-models"
              placeholder={MODEL_PLACEHOLDER}
              value={draft.modelName}
              onChange={(event) => update({ modelName: event.target.value })}
              className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
            />
            <datalist id="assistant-known-byok-models">
              {BYOK_MODEL_SUGGESTIONS.map((model) => (
                <option key={model} value={model} />
              ))}
            </datalist>
          </>
        )}
      </FieldScaffold>

      <Input
        label={API_KEY_LABEL}
        type="password"
        placeholder={API_KEY_PLACEHOLDER}
        value={draft.apiKey}
        onChange={(event) => update({ apiKey: event.target.value })}
      />

      {error !== null ? (
        <p role="alert" className="text-body text-(--danger)">
          {ADD_FAILED_TITLE}: {error === "" ? ADD_FAILED_FALLBACK : error}
        </p>
      ) : null}

      <div>
        <Button type="submit" variant="primary" disabled={!enabled}>
          {saving ? ADD_SAVING_ACTION : ADD_ACTION}
        </Button>
      </div>
    </form>
  );
}
