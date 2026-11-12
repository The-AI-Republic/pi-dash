// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page assistant task call (parity/specs/pages.md §9.3, PAGE-075). One
// conversational task at workspace scope: the composed instruction rides
// `task`, the snapshotted selection rides `prompt`, and the answer keeps
// the plain text plus the review-ready markup. Only this task ships; the
// unrouted grammar endpoint and the unused summary endpoint stay
// uncalled (spec §16).

import { runWorkspaceEditorTask, type ApiClient } from "@pidash/api-client";
import { useMutation } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";

/** An assistant answer: the plain text plus the review-ready markup. */
export interface PageAssistantAnswer {
  text: string;
  html: string;
}

/**
 * Run one assistant task and keep the text plus the markup it answers.
 * Both forms are trimmed; an empty answer is a valid answer the panel
 * reports, not a failure.
 */
export async function requestPageAssistantAnswer(
  client: ApiClient,
  workspaceSlug: string,
  task: string,
  prompt: string
): Promise<PageAssistantAnswer> {
  const answer = await runWorkspaceEditorTask(client, workspaceSlug, { task, prompt });
  return { text: answer.response.trim(), html: answer.response_html.trim() };
}

/** Post one assistant task for the workspace (PAGE-075). */
export function usePageAssistantAnswer(workspaceSlug: string) {
  return useMutation<PageAssistantAnswer, Error, { task: string; prompt: string }>({
    mutationFn: ({ task, prompt }) => requestPageAssistantAnswer(getAppClient(), workspaceSlug, task, prompt),
  });
}
