// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace settings helpers (ADM-037–042, from
// parity/specs/admin.md §5): the creation-restriction flag codec, the
// parity-pinned configuration notice copy, and the cache merge saved
// forms re-read. Pure for tests; the hooks in hooks.ts wire them to
// the configuration query and the save mutation.
import type { InstanceConfigurationList } from "@pidash/api-client";

/** Configuration key gating who may create workspaces (§5.1). */
export const DISABLE_WORKSPACE_CREATION_KEY = "DISABLE_WORKSPACE_CREATION";

/** Progress notice while a configuration save is in flight. */
export const CONFIG_SAVE_PROGRESS_TITLE = "Saving configuration";

/** Success notice once a configuration save lands (parity-pinned). */
export const CONFIG_SAVE_SUCCESS_TITLE = "Success";
export const CONFIG_SAVE_SUCCESS_MESSAGE = "Configuration saved successfully";

/** Failure notice when a configuration save fails (parity-pinned). */
export const CONFIG_SAVE_FAILURE_TITLE = "Error";
export const CONFIG_SAVE_FAILURE_MESSAGE = "Failed to save configuration";

/**
 * Whether workspace creation is restricted for the stored flag value:
 * numeric parse, nonzero is on, missing or unparseable is off (§0.6).
 */
export function isWorkspaceCreationRestricted(value: string | null | undefined): boolean {
  if (value === null || value === undefined) return false;
  const parsed = Number.parseInt(value, 10);
  return !Number.isNaN(parsed) && parsed !== 0;
}

/** Encode the restriction state back to the stored string flag. */
export function encodeWorkspaceCreationRestricted(restricted: boolean): "1" | "0" {
  return restricted ? "1" : "0";
}

/**
 * Fold saved configuration rows into the cached key map (re-exported
 * from the shared query module, which owns the single implementation
 * every settings page's save path uses).
 */
export { mergeConfigurationRows } from "./queries.js";

/** Read one raw configuration value out of the key map, if present. */
export function selectConfigurationValue(
  rows: InstanceConfigurationList | undefined,
  key: string
): string | null | undefined {
  return rows?.find((row) => row.key === key)?.value;
}
