// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add a connection (parity/specs/cloud.md §7, CLOUD-033). The key form asks
// for a provider, a base address for the compatible kind, and a secret; a
// new key needs at least one typed model ID (NEWPIDASH-394, design section
// 7.2), listed with add and remove. The server is the authority on
// reachability, so a refused save stays in the form under the fields: the
// provider's message lands beside each ID, and unverified-only results
// offer "Save anyway" instead of clearing them. Provider sign-in lives on
// the row that needs it, not here.
import type { ModelTestResults } from "@pidash/api-client";
import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { ModelIdEditor, modelFailedIds, modelFailureDetails, saveAnywayModels } from "../../assistant/index.js";
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
  PROVIDER_LABEL,
} from "../lib/copy.js";
import {
  BYOK_PROVIDERS,
  EMPTY_BYOK_DRAFT,
  byokDraftValid,
  needsBaseUrl,
  type ByokDraft,
  type ByokProvider,
} from "../lib/byok.js";

export interface AddConnectionViewProps {
  saving: boolean;
  error: string | null;
  /** Per-ID results of the last refused save; null while the last save passed. */
  results: ModelTestResults | null;
  onSave(draft: ByokDraft, saveUnverified: boolean): void;
}

export function AddConnectionView({ saving, error, results, onSave }: AddConnectionViewProps): React.ReactElement {
  const [draft, setDraft] = React.useState<ByokDraft>(EMPTY_BYOK_DRAFT);
  const baseVisible = needsBaseUrl(draft.provider);
  const enabled = !saving && byokDraftValid(draft);
  const failures = React.useMemo(() => (results === null ? {} : modelFailureDetails(results)), [results]);
  const failedIds = React.useMemo(() => (results === null ? [] : modelFailedIds(results)), [results]);

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
        onSave(draft, false);
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

      <ModelIdEditor
        ids={draft.models}
        verifiedAt={{}}
        failures={failures}
        failedIds={failedIds}
        pendingIds={[]}
        disabled={saving}
        testable={false}
        confirmLastRemove={false}
        keyLabel={draft.provider === "anthropic" ? ANTHROPIC_PROVIDER_LABEL : COMPATIBLE_PROVIDER_LABEL}
        onAdd={(model) => update({ models: [...draft.models, model] })}
        onRemove={(model) => update({ models: draft.models.filter((kept) => kept !== model) })}
        onTest={() => undefined}
        onSaveAnyway={results !== null && saveAnywayModels(results) !== null ? () => onSave(draft, true) : null}
      />

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
