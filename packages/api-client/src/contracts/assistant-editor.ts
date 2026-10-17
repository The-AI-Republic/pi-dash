// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor AI tasks: the writing helper's task posts. The workspace
// task answers text plus markup; the project (issue-modal) task adds
// the project and workspace details. The rephrase post has no
// backend in this checkout — its shapes are contracted for the
// target, and the live absence stays locked by the contract suite.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

export interface EditorTaskInput {
  task: string;
  prompt?: string;
}

/** The generated text, raw and review-ready markup. */
export const EditorTaskResponse = z.object({
  response: z.string(),
  response_html: z.string(),
});
export type EditorTaskResponse = z.infer<typeof EditorTaskResponse>;

/** Run an editor task at workspace scope (page editor use). */
export async function runWorkspaceEditorTask(
  client: ApiClient,
  slug: string,
  input: EditorTaskInput
): Promise<EditorTaskResponse> {
  const path = `/api/workspaces/${slug}/ai-assistant/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { task: input.task, prompt: input.prompt },
  });
  return client.parse(EditorTaskResponse, envelope.data, `POST ${path}`);
}

/** The project detail the issue-modal task echoes. */
export const EditorTaskProjectDetail = z.object({
  id: uuid(),
  identifier: z.string(),
  name: z.string(),
  cover_image: z.nullable(z.string()),
  cover_image_url: z.nullable(z.string()),
  logo_props: z.record(z.string(), z.unknown()),
  description: z.string(),
  is_default: z.boolean(),
});
export type EditorTaskProjectDetail = z.infer<typeof EditorTaskProjectDetail>;

/** The workspace detail the issue-modal task echoes. */
export const EditorTaskWorkspaceDetail = z.object({
  name: z.string(),
  slug: z.string(),
  id: uuid(),
  logo_url: z.nullable(z.string()),
});
export type EditorTaskWorkspaceDetail = z.infer<typeof EditorTaskWorkspaceDetail>;

/** The issue-modal answer: text plus the echoed details. */
export const ProjectEditorTaskResponse = z.object({
  response: z.string(),
  response_html: z.string(),
  project_detail: EditorTaskProjectDetail,
  workspace_detail: EditorTaskWorkspaceDetail,
});
export type ProjectEditorTaskResponse = z.infer<typeof ProjectEditorTaskResponse>;

/** Run an editor task for the issue-modal draft (no prompt sent). */
export async function runProjectEditorTask(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: EditorTaskInput
): Promise<ProjectEditorTaskResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/ai-assistant/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { task: input.task, prompt: input.prompt },
  });
  return client.parse(ProjectEditorTaskResponse, envelope.data, `POST ${path}`);
}

export interface RephraseGrammarInput {
  task: string;
  text_input: string;
  casual_score?: number;
  formal_score?: number;
}

/** The rephrased text the grammar post answers. */
export const RephraseGrammarResponse = z.object({
  response: z.string(),
});
export type RephraseGrammarResponse = z.infer<typeof RephraseGrammarResponse>;

/**
 * Rephrase or fix the grammar of editor text. No backend answers in
 * this checkout — the call rejects with the platform 404 until the
 * target backend lands.
 */
export async function rephraseGrammar(
  client: ApiClient,
  slug: string,
  input: RephraseGrammarInput
): Promise<RephraseGrammarResponse> {
  const path = `/api/workspaces/${slug}/rephrase-grammar/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      task: input.task,
      text_input: input.text_input,
      casual_score: input.casual_score,
      formal_score: input.formal_score,
    },
  });
  return client.parse(RephraseGrammarResponse, envelope.data, `POST ${path}`);
}
