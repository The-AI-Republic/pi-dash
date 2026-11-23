// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-supplied key form rules (parity/specs/cloud.md §7, CLOUD-033). The
// form enables save only when the assistant has everything it needs: a
// provider, a base address for the compatible kind, a model and a secret.
// Validation is a pure shape check — reachability is the server's call and
// its refusal lands in the form's error slot unchanged.

import { BYOK_MISSING_BASE, BYOK_MISSING_KEY, BYOK_MISSING_MODEL } from "./copy.js";

/** Provider kinds the key form offers, in display order. */
export const BYOK_PROVIDERS = ["openai_compatible", "anthropic"] as const;
export type ByokProvider = (typeof BYOK_PROVIDERS)[number];

export interface ByokDraft {
  provider: ByokProvider;
  baseUrl: string;
  /** The key's typed model IDs (NEWPIDASH-394, design section 7.2); a new key needs at least one. */
  models: string[];
  apiKey: string;
}

export const EMPTY_BYOK_DRAFT: ByokDraft = {
  provider: "openai_compatible",
  baseUrl: "",
  models: [],
  apiKey: "",
};

/** A custom endpoint needs its own address; a single vendor answers on its own. */
export function needsBaseUrl(provider: ByokProvider): boolean {
  return provider === "openai_compatible";
}

export interface ByokProblems {
  baseUrl: string | null;
  models: string | null;
  apiKey: string | null;
}

export function validateByokDraft(draft: ByokDraft): ByokProblems {
  return {
    baseUrl: needsBaseUrl(draft.provider) && draft.baseUrl.trim() === "" ? BYOK_MISSING_BASE : null,
    models: draft.models.length === 0 ? BYOK_MISSING_MODEL : null,
    apiKey: draft.apiKey.trim() === "" ? BYOK_MISSING_KEY : null,
  };
}

export function byokDraftValid(draft: ByokDraft): boolean {
  return Object.values(validateByokDraft(draft)).every((problem) => problem === null);
}

export interface ByokSubmission {
  providerKind: ByokProvider;
  baseUrl: string | null;
  /** First ID; older servers read the key's single model from it. */
  modelName: string;
  /** The key's full model-ID list (design section 6.3). */
  models: string[];
  apiKey: string;
}

/** The trimmed values a valid draft sends; the base stays off other kinds. */
export function byokSubmission(draft: ByokDraft): ByokSubmission {
  return {
    providerKind: draft.provider,
    baseUrl: needsBaseUrl(draft.provider) ? draft.baseUrl.trim() : null,
    modelName: draft.models[0] ?? "",
    models: draft.models,
    apiKey: draft.apiKey.trim(),
  };
}
