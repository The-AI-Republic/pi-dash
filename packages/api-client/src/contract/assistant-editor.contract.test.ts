// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: editor task posts against live Django. The contract
// stack configures no provider key, so both task posts answer the
// missing-key refusal; the rephrase post has no backend at all and
// locks the platform 404. Success shapes stay unit-stubbed.
import { describe, expect, it } from "vitest";
import { rephraseGrammar, runProjectEditorTask, runWorkspaceEditorTask } from "../contracts/assistant-editor.js";
import { assistantErrorFrom } from "../contracts/assistant-errors.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import { isApiError } from "../errors.js";
import { listProjects } from "../contracts/projects.js";

describe.skipIf(!contractEnabled)("assistant editor contract", () => {
  it("refuses the workspace task without a provider key", async () => {
    const client = await sharedSignedIn();
    const refused: unknown = await runWorkspaceEditorTask(client, CONTRACT_WORKSPACE, {
      task: "Polish",
      prompt: "a draft",
    }).then(
      () => null,
      (error: unknown) => error
    );
    expect(assistantErrorFrom(refused)).toMatchObject({
      code: "LLM provider API key and model are required",
      status: 400,
    });
  });

  it("refuses the project task without a provider key", async () => {
    const client = await sharedSignedIn();
    const projects = await listProjects(client, CONTRACT_WORKSPACE);
    const project = projects.find((row) => row.identifier === CONTRACT_PROJECT_IDENTIFIER);
    expect(project?.id).toBeDefined();
    if (!project) return;
    const refused: unknown = await runProjectEditorTask(client, CONTRACT_WORKSPACE, project.id, {
      task: "Polish",
    }).then(
      () => null,
      (error: unknown) => error
    );
    expect(assistantErrorFrom(refused)).toMatchObject({
      code: "LLM provider API key and model are required",
      status: 400,
    });
  });

  it("locks the rephrase absence as a 404", async () => {
    const client = await sharedSignedIn();
    const refused: unknown = await rephraseGrammar(client, CONTRACT_WORKSPACE, {
      task: "ASK_ANYTHING",
      text_input: "teh draft",
    }).then(
      () => null,
      (error: unknown) => error
    );
    // No backend answers in this checkout (NEWFRONT-206): only the
    // status is asserted — the body is the debug HTML page here and
    // the JSON platform error with DEBUG off.
    expect(isApiError(refused) && refused.status).toBe(404);
  });
});
