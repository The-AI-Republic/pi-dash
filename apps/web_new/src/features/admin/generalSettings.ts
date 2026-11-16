// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General settings pure logic (ADM-013–017, from
// parity/specs/admin.md §2): name validation, the first-admin email
// read, the telemetry prefill, and folding a save answer back into
// the cached bootstrap. The save labels, the success notice, the
// failure console line, and the policy link are pinned by the parity
// suite; the required-field message is fresh (NEWFRONT-475 intent).
import type { InstanceAdminList, InstanceBootstrap, InstanceDetails } from "@pidash/api-client";

/** Idle save label, pinned by the parity suite. */
export const GENERAL_SAVE_IDLE_LABEL = "Save changes";

/** In-flight save label, pinned by the parity suite. */
export const GENERAL_SAVE_BUSY_LABEL = "Saving";

/** Success notice title, pinned by the parity suite. */
export const GENERAL_SAVE_SUCCESS_TITLE = "Success";

/** Success notice message, pinned by the parity suite. */
export const GENERAL_SAVE_SUCCESS_MESSAGE = "Settings updated successfully";

/**
 * Failure console line for the log-only save quirk (ADM-017), pinned
 * by the parity suite. Failures log here and show no notice.
 */
export const GENERAL_SAVE_FAILURE_LOG = "Error updating the instance info";

/** Telemetry policy link target, pinned by the parity suite. */
export const GENERAL_TELEMETRY_DOC_URL = "https://github.com/The-AI-Republic/pi-dash#readme";

/** Inline message when an empty name blocks the save (NEWFRONT-475). */
export const GENERAL_NAME_REQUIRED_MESSAGE = "Instance name is required.";

/** A name saves only when it holds a non-blank value. */
export function isGeneralNameValid(name: string): boolean {
  return name.trim().length > 0;
}

/**
 * The first admin's nested user email for the read-only field
 * (ADM-014); blank until the list resolves or when it holds none.
 */
export function firstAdminEmail(admins: InstanceAdminList | undefined): string {
  return admins?.[0]?.user_detail.email ?? "";
}

/**
 * Telemetry prefill (ADM-016): the stored flag, off when unset. The
 * unset branch is unreachable in practice — the flag is a
 * non-nullable boolean — and is pinned by unit tests only.
 */
export function resolveTelemetryPrefill(value: boolean | null | undefined): boolean {
  return value ?? false;
}

/**
 * Fold a general-settings save answer into the cached bootstrap: the
 * PATCH answers the stored row without the computed workspace flag,
 * so the cached flag carries over untouched.
 */
export function mergeGeneralSettingsIntoBootstrap(
  current: InstanceBootstrap,
  updated: InstanceDetails
): InstanceBootstrap {
  return { ...current, instance: { ...updated, workspaces_exist: current.instance.workspaces_exist } };
}
