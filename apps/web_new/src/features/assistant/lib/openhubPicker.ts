// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub composer picker state (NEWPIDASH-303, rewritten NEWPIDASH-394).
// Pure on purpose — the merged list and the option labels are
// unit-testable without a DOM or a server. The component reads the
// connections list (god-mode order via openhub_model_options,
// NEWPIDASH-302) and the per-surface selection, so an admin-added model
// appears after reload with no deploy.
//
// Merged picker (NEWPIDASH-394, design section 7.1): catalog models
// first, then BYOK models grouped by key. The agent surface takes
// catalog models only. Quota badges ride on top later (annotateChoices
// in cloud-assistant); this builder stays quota-free.

import type { AssistantConnectionList } from "@pidash/api-client";

import { baseUrlHost, OPENHUB_MODEL_DESCRIPTIONS, type ModelChoice } from "../../cloud-assistant/index.js";

/** The hosted lane's free/default model (god-mode preselected default). */
export const OPENHUB_DEFAULT_MODEL = "openhub_free_llm";

/** Suffix marking the default model in the picker (NEWPIDASH-303). */
export const OPENHUB_DEFAULT_SUFFIX = " (default)";

/**
 * The catalog half of the merged list, read off the connections list:
 * god-mode order with the server labels when the backend sends them
 * (NEWPIDASH-393, design section 6.2).
 */
export function catalogFromList(list: AssistantConnectionList): MergedPickerCatalog {
  return {
    models: list.openhub_model_options,
    labels: list.openhub_model_labels ?? {},
    descriptions: OPENHUB_MODEL_DESCRIPTIONS,
  };
}

/**
 * Every BYOK key's contribution, in list order. A connection without a
 * stored list still serves its legacy single model, so it contributes
 * that one ID.
 */
export function byokKeysFromList(list: AssistantConnectionList): MergedPickerByokKey[] {
  return list.connections
    .filter((row) => row.kind === "byok")
    .map((row) => ({
      connectionId: row.id,
      models: row.models ?? (row.model_name === "" ? [] : [row.model_name]),
      baseUrl: row.base_url,
      providerKind: row.provider_kind,
    }));
}

/**
 * One option's visible text: the label, its credit badge when the quota
 * answer stamped one, and the default mark on the free model. The default
 * mark is what points a credit-blocked turn back at the free model
 * (the switch-to-free affordance NEWPIDASH-303 asks for).
 */
export function pickerOptionLabel(choice: ModelChoice, isDefault: boolean): string {
  const base = choice.badge === null ? choice.label : `${choice.label} (${choice.badge})`;
  return isDefault ? `${base}${OPENHUB_DEFAULT_SUFFIX}` : base;
}

/** The catalog half of the merged list: admin order plus label sources. */
export interface MergedPickerCatalog {
  /** Catalog models in catalog order. */
  models: readonly string[];
  /** Server catalog labels (`openhub_model_labels`), keyed by model ID. */
  labels: Readonly<Record<string, string>>;
  /** Built-in descriptions, keyed by model ID. */
  descriptions: Readonly<Record<string, string>>;
}

/** One BYOK key's contribution to the merged list. */
export interface MergedPickerByokKey {
  /** The connection whose key runs these models. */
  connectionId: string;
  /** The key's typed model IDs, in stored order. */
  models: readonly string[];
  baseUrl: string;
  providerKind: string;
}

/**
 * One merged option: the shared choice shape plus where it came from.
 * The picker writes a BYOK choice with its `connectionId`; a catalog
 * choice with none.
 */
export interface MergedPickerChoice extends ModelChoice {
  origin: "catalog" | "byok";
  connectionId?: string;
}

/**
 * The merged list (design section 7.1): catalog models in catalog order
 * (label, else description, else ID), then each BYOK key in order with
 * its model IDs. Codex models never enter — the caller passes only the
 * catalog and BYOK keys. `includeByok: false` renders the agent
 * surface's catalog-only list. Every entry stays selectable here; the
 * free-plan credit rule stamps later through `annotateChoices`.
 */
export function buildMergedModelChoices(
  catalog: MergedPickerCatalog,
  keys: readonly MergedPickerByokKey[],
  options: { includeByok?: boolean } = {}
): MergedPickerChoice[] {
  const catalogChoices: MergedPickerChoice[] = catalog.models.map((model) => ({
    value: model,
    label: catalog.labels[model] ?? catalog.descriptions[model] ?? model,
    credit: null,
    badge: null,
    selectable: true,
    origin: "catalog" as const,
  }));
  if (options.includeByok === false) return catalogChoices;
  return catalogChoices.concat(
    keys.flatMap((key) =>
      key.models.map((model) => ({
        value: model,
        label: byokChoiceLabel(model, key),
        credit: null,
        badge: null,
        selectable: true,
        origin: "byok" as const,
        connectionId: key.connectionId,
      }))
    )
  );
}

/**
 * One BYOK option's text: `<id> (BYOK · <base-URL host>)`, or `<id>
 * (BYOK · <provider>)` when the key has no base URL. An unparseable
 * address falls back to the provider, never to raw input.
 */
export function byokChoiceLabel(model: string, key: Pick<MergedPickerByokKey, "baseUrl" | "providerKind">): string {
  return `${model} (BYOK · ${baseUrlHost(key.baseUrl) ?? key.providerKind})`;
}

/**
 * The one-line fallback notice the picker and Settings share: shown
 * when `fallback_reason` is set and a saved choice exists, e.g.
 * "modelC is no longer available. Using OpenHub free model."
 */
export function fallbackNotice(savedModel: string, effectiveLabel: string): string {
  return `${savedModel} is no longer available. Using ${effectiveLabel}.`;
}

/**
 * One merged option's `<select>` value. The same model ID through two
 * connections is two choices (design D-10), so a BYOK choice carries
 * its connection ID; catalog choices carry none.
 */
export function mergedChoiceKey(choice: { value: string; connectionId?: string | undefined }): string {
  return choice.connectionId === undefined ? `catalog:${choice.value}` : `byok:${choice.connectionId}:${choice.value}`;
}
