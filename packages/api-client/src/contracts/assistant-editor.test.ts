// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor task posts against a stub client (no backend — success
// needs a provider key the contract stack never configures).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { rephraseGrammar, runProjectEditorTask, runWorkspaceEditorTask } from "./assistant-editor.js";

const PROJECT = "123e4567-e89b-12d3-a456-426614174000";
const WORKSPACE = "223e4567-e89b-12d3-a456-426614174001";

function stubClient(impl: {
  getJson?: ApiClient["getJson"];
  sendJson?: ApiClient["sendJson"];
  post?: ApiClient["post"];
  delete?: ApiClient["delete"];
}): ApiClient {
  return {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    ...impl,
  } as ApiClient;
}

describe("editor tasks", () => {
  it("posts the workspace task with the typed text", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { response: "polished", response_html: "polished" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const answer = await runWorkspaceEditorTask(client, "acme", { task: "Polish", prompt: "draft" });
    expect(calls[0]?.url).toBe("/api/workspaces/acme/ai-assistant/");
    expect(calls[0]?.json).toEqual({ task: "Polish", prompt: "draft" });
    expect(answer.response).toBe("polished");
  });

  it("posts the project task and parses the echoed details", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return {
          data: {
            response: "polished",
            response_html: "polished",
            project_detail: {
              id: PROJECT,
              identifier: "CT",
              name: "Contract",
              cover_image: null,
              cover_image_url: null,
              logo_props: {},
              description: "",
              is_default: false,
            },
            workspace_detail: { name: "Acme", slug: "acme", id: WORKSPACE, logo_url: null },
          },
          status: 200,
          url,
        };
      }) as ApiClient["sendJson"],
    });
    const answer = await runProjectEditorTask(client, "acme", PROJECT, { task: "Polish" });
    expect(calls[0]?.url).toBe(`/api/workspaces/acme/projects/${PROJECT}/ai-assistant/`);
    expect(calls[0]?.json).toEqual({ task: "Polish", prompt: undefined });
    expect(answer.project_detail.identifier).toBe("CT");
    expect(answer.workspace_detail.slug).toBe("acme");
  });

  it("posts the rephrase task with the text and scores", async () => {
    const calls: { url?: string; json?: unknown }[] = [];
    const client = stubClient({
      sendJson: (async (_method: string, url: string, options: { json?: unknown }) => {
        calls.push({ url, json: options.json });
        return { data: { response: "fixed" }, status: 200, url };
      }) as ApiClient["sendJson"],
    });
    const answer = await rephraseGrammar(client, "acme", {
      task: "ASK_ANYTHING",
      text_input: "teh draft",
      casual_score: 3,
      formal_score: 7,
    });
    expect(calls[0]?.url).toBe("/api/workspaces/acme/rephrase-grammar/");
    expect(calls[0]?.json).toEqual({ task: "ASK_ANYTHING", text_input: "teh draft", casual_score: 3, formal_score: 7 });
    expect(answer.response).toBe("fixed");
  });
});
