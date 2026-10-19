// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod form rules (NEWFRONT-542, RUN-013/RUN-014/RUN-015). A pod name is
// stored with its project's identifier in front, so every dialog edits the
// bare suffix and lets the server put the prefix back. The update payload
// is the difference between the form and the stored row — an unchanged
// form sends nothing at all.

import { isApiError, type Pod, type PodPatch } from "@pidash/api-client";

/** Separator the server inserts between the project identifier and the suffix. */
export const POD_NAME_SEPARATOR = "_";

/** Problem texts a name can raise before the server sees it. */
export const POD_NAME_REQUIRED = "Name is required.";
export const POD_PROJECT_REQUIRED = "Pick a project.";

/** Something went wrong, shown as the toast title (parity pins this word). */
export const POD_ERROR_TITLE = "Error!";
export const POD_CREATE_FAILED = "Could not create the pod.";
export const POD_UPDATE_FAILED = "Could not update the pod.";
export const POD_DELETE_FAILED = "Failed to delete pod";

/**
 * The editable fraction of a pod name: everything after `{identifier}_`.
 * A name without the prefix (a project renamed since creation) is shown
 * whole rather than mangled.
 */
export function podNameSuffix(name: string, projectIdentifier: string): string {
  const prefix = `${projectIdentifier}${POD_NAME_SEPARATOR}`;
  return name.startsWith(prefix) ? name.slice(prefix.length) : name;
}

/** The stored name a suffix produces once the server re-prefixes it. */
export function podNameWithPrefix(suffix: string, projectIdentifier: string): string {
  return `${projectIdentifier}${POD_NAME_SEPARATOR}${suffix}`;
}

/** Trimmed name, or the message that blocks submission. */
export function validatePodName(raw: string): { value: string } | { error: string } {
  const value = raw.trim();
  return value === "" ? { error: POD_NAME_REQUIRED } : { value };
}

/** The create-pod body; `description` is omitted when left blank. */
export interface PodDraft {
  projectId: string;
  name: string;
  description: string;
}

export type PodDraftErrors = Partial<Record<"projectId" | "name", string>>;

/** Field messages for a create submission; empty means it may be sent. */
export function validatePodDraft(draft: Pick<PodDraft, "projectId" | "name">): PodDraftErrors {
  const errors: PodDraftErrors = {};
  if (draft.projectId.trim() === "") errors.projectId = POD_PROJECT_REQUIRED;
  const name = validatePodName(draft.name);
  if ("error" in name) errors.name = name.error;
  return errors;
}

/** The stored values an edit dialog starts from. */
export interface PodEditSeed {
  name: string;
  description: string;
  isDefault: boolean;
}

export function seedPodEdit(pod: Pod): PodEditSeed {
  return {
    name: podNameSuffix(pod.name, pod.project_identifier),
    description: pod.description,
    isDefault: pod.is_default,
  };
}

/**
 * The PATCH body for an edited pod, or `null` when nothing changed. Names
 * and descriptions compare trimmed; a blank description is sent as such so
 * the user can clear one.
 */
export function podUpdatePatch(seed: PodEditSeed, draft: PodEditSeed): PodPatch | null {
  const patch: PodPatch = {};
  const name = draft.name.trim();
  if (name !== seed.name.trim()) patch.name = name;
  const description = draft.description.trim();
  if (description !== seed.description.trim()) patch.description = description;
  if (draft.isDefault !== seed.isDefault) patch.is_default = draft.isDefault;
  return Object.keys(patch).length === 0 ? null : patch;
}

/** Why the server refused a pod deletion, in the words a person can act on. */
export const POD_DELETE_GUARDS: Readonly<Record<string, string>> = {
  pod_has_runners: "This pod's runners must be moved or revoked before it can be deleted.",
  pod_has_active_runs: "This pod has runs in flight. Cancel them or wait for them to finish before deleting it.",
  default_pod_undeletable: "This is the project's default pod. Make another pod the default first.",
};

/**
 * The sentence to show for a failed pod deletion: the guard that fired
 * when we recognize its code, otherwise whatever the server said, and
 * finally the generic message.
 */
export function podDeleteErrorMessage(error: unknown): string {
  if (isApiError(error)) {
    const guard = POD_DELETE_GUARDS[error.code];
    if (guard !== undefined) return guard;
    if (error.message.trim() !== "") return error.message;
  }
  return POD_DELETE_FAILED;
}

/** The same choice for create and edit failures. */
export function podWriteErrorMessage(error: unknown, fallback: string): string {
  if (isApiError(error) && error.message.trim() !== "") return error.message;
  return fallback;
}
