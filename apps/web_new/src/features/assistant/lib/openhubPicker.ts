// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub composer picker state (NEWPIDASH-303). Pure on purpose — the
// visibility gate and the option labels are unit-testable without a DOM
// or a server. The component reads the connections list (god-mode order
// via openhub_model_options, NEWPIDASH-302) and the LLM config
// (credential lane), so an admin-added model appears after reload with
// no deploy.

import { resolveCredentialLane, type AssistantConnectionList, type LlmConfig } from "@pidash/api-client";

import type { ModelChoice } from "../../cloud-assistant/index.js";

/** The hosted lane's free/default model (god-mode preselected default). */
export const OPENHUB_DEFAULT_MODEL = "openhub_free_llm";

/** Suffix marking the default model in the picker (NEWPIDASH-303). */
export const OPENHUB_DEFAULT_SUFFIX = " (default)";

/**
 * The selected OpenHub connection, when the lane is OpenHub and usable.
 * Bring-your-own-key and Codex lanes hide the picker (their model controls
 * stay in Settings), as does a keyless or unloaded config.
 */
export function selectedOpenHubConnection(
  config: LlmConfig | null | undefined,
  list: AssistantConnectionList | undefined
): AssistantConnectionList["connections"][number] | null {
  if (config === null || config === undefined || list === undefined) return null;
  if (resolveCredentialLane(config) !== "openhub") return null;
  if (config.has_openhub_access !== true) return null;
  return list.connections.find((row) => row.kind === "openhub" && row.is_selected) ?? null;
}

/** True when the composer picker should render. */
export function shouldShowOpenHubPicker(
  config: LlmConfig | null | undefined,
  list: AssistantConnectionList | undefined
): boolean {
  return selectedOpenHubConnection(config, list) !== null;
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
