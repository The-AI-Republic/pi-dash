// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompt section writes (parity/specs/agents.md §6, AGT-024, AGT-025,
// AGT-027). Upserts persist the draft at one scope, reverts deactivate
// the override there; both toast and refresh the sections subtree. Save
// failures surface the server's detail inline; revert failures report
// only the generic message, dropping the server detail.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  isApiError,
  previewPrompt,
  revertPromptSection,
  upsertPromptSection,
  type PromptPreview,
  type PromptPreviewInput,
  type PromptSectionOverride,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifySuccess } from "../../../shared/shell/index.js";
import type { PromptEditorScope, PromptKind } from "../lib/sections.js";
import { promptSectionKeys } from "./keys.js";

/** Success title after the workspace default lands. */
export const WORKSPACE_SAVED_TITLE = "Workspace default saved";
const WORKSPACE_SAVED_BODY = "Members without their own override now see the new default.";
/** Success title after the personal override lands. */
export const PERSONAL_SAVED_TITLE = "Personal override saved";
const PERSONAL_SAVED_BODY = "Runs you trigger now use this text.";
/** Success title after a revert lands. */
export const SECTION_REVERTED_TITLE = "Section reverted";
const WORKSPACE_REVERTED_BODY = "The workspace default is gone; sections fall back to the registry default.";
const PERSONAL_REVERTED_BODY = "Your override is gone; your runs fall back to the workspace default.";
/** Generic inline message for a refused revert (the oracle reads it verbatim). */
export const REVERT_FAILED_MESSAGE = "Could not revert the section.";
/** Framing for an inline save failure; the server detail follows it. */
const SAVE_FAILED_LEAD = "Could not save the section.";
/** Last-resort detail when the failure carries no message at all. */
const SAVE_FAILED_FALLBACK = "The server gave no reason.";

/**
 * Pull the human-readable detail out of a failed save: the first field
 * message for validation refusals, the normalized message otherwise.
 */
export function saveFailureDetail(error: unknown): string {
  if (isApiError(error)) {
    if (error.fields !== undefined) {
      for (const messages of Object.values(error.fields)) {
        const first = messages[0];
        if (first !== undefined) return first;
      }
    }
    if (error.message !== "") return error.message;
    return SAVE_FAILED_FALLBACK;
  }
  if (error instanceof Error && error.message !== "") return error.message;
  return SAVE_FAILED_FALLBACK;
}

/** The inline save error: our framing plus the server's detail. */
export function saveFailureMessage(error: unknown): string {
  return `${SAVE_FAILED_LEAD} ${saveFailureDetail(error)}`;
}

export interface PromptUpsertInput {
  key: string;
  scope: PromptEditorScope;
  body: string;
}

/** Persist the draft at one scope, toasting and refreshing after. */
export function useUpsertPromptSection(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<PromptSectionOverride, Error, PromptUpsertInput>({
    mutationFn: (input) =>
      upsertPromptSection(getAppClient(), workspaceSlug, input.key, { scope: input.scope, body: input.body }),
    onSuccess: (_override, input) => {
      if (input.scope === "workspace") {
        notifySuccess(WORKSPACE_SAVED_TITLE, WORKSPACE_SAVED_BODY);
      } else {
        notifySuccess(PERSONAL_SAVED_TITLE, PERSONAL_SAVED_BODY);
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: promptSectionKeys.all(workspaceSlug) });
    },
  });
}

export interface PromptRevertInput {
  key: string;
  scope: PromptEditorScope;
}

/** Deactivate the override at one scope, toasting and refreshing after. */
export function useRevertPromptSection(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<number, Error, PromptRevertInput>({
    mutationFn: (input) => revertPromptSection(getAppClient(), workspaceSlug, input.key, input.scope),
    onSuccess: (_status, input) => {
      notifySuccess(
        SECTION_REVERTED_TITLE,
        input.scope === "workspace" ? WORKSPACE_REVERTED_BODY : PERSONAL_REVERTED_BODY
      );
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: promptSectionKeys.all(workspaceSlug) });
    },
  });
}

/** Generic inline message when a render fails with no usable detail. */
export const PREVIEW_FAILED_FALLBACK = "The prompt could not be rendered.";
/** Inline refusal when the target field is blank (submit stays disarmed too). */
export const MISSING_ISSUE_TARGET = "Enter an issue id first.";
/** Inline refusal when the install field is blank for the scheduler kind. */
export const MISSING_INSTALL_TARGET = "Enter an install id first.";

/** The inline preview error: the server's message, or our generic fallback. */
export function previewFailureMessage(error: unknown): string {
  if (isApiError(error)) {
    if (error.fields !== undefined) {
      for (const messages of Object.values(error.fields)) {
        const first = messages[0];
        if (first !== undefined) return first;
      }
    }
    if (error.message !== "") return error.message;
    return PREVIEW_FAILED_FALLBACK;
  }
  if (error instanceof Error && error.message !== "") return error.message;
  return PREVIEW_FAILED_FALLBACK;
}

export interface PromptPreviewRequest {
  kind: PromptKind;
  input: PromptPreviewInput;
}

/** Render a template against a real issue or install, without creating a run. */
export function usePreviewPrompt(workspaceSlug: string) {
  return useMutation<PromptPreview, Error, PromptPreviewRequest>({
    mutationFn: (request) => previewPrompt(getAppClient(), workspaceSlug, request.kind, request.input),
  });
}
