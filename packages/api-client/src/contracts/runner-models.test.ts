// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner model catalog contract unit tests against a stub client: the
// request surface (paths, methods, bodies) and the parse branches the
// live pytest suite (runner_models/) pins against the Rust binary.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  RunnerModelCatalogResponse,
  RunnerModelRow,
  createRunnerModel,
  deleteRunnerModel,
  getModelCatalog,
  listAdminRunnerModels,
  updateRunnerModel,
} from "./runner-models.js";

interface Call {
  method?: string;
  url?: string;
  json?: unknown;
}

function row(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    agent: "codex",
    model: "gpt-5.5",
    label: "GPT-5.5",
    reasoning_efforts: ["low", "medium", "high", "xhigh"],
    sort_order: 4,
    is_enabled: true,
    is_preselected: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function grouped(agents: Record<string, unknown[]>): Record<string, unknown> {
  return { agents };
}

function stubClient(responses: Map<string, unknown>, calls: Call[] = []): ApiClient {
  return {
    getJson: (async (url: string) => {
      calls.push({ method: "GET", url });
      const body = responses.get(url);
      if (body === undefined) throw new Error(`unstubbed GET ${url}`);
      return body;
    }) as ApiClient["getJson"],
    sendJson: (async (method: string, url: string, options?: { json?: unknown }) => {
      calls.push({ method, url, json: options?.json });
      const body = responses.get(url);
      if (body === undefined) throw new Error(`unstubbed ${method} ${url}`);
      return { data: body, status: method === "POST" ? 201 : 200, url };
    }) as ApiClient["sendJson"],
    delete: (async (url: string) => {
      calls.push({ method: "DELETE", url });
      return { status: 204, url, headers: { get: () => null }, text: () => Promise.resolve("") };
    }) as ApiClient["delete"],
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
  } as ApiClient;
}

describe("runner model catalog contracts", () => {
  it("reads the public catalog from the signed-in path", async () => {
    const calls: Call[] = [];
    const client = stubClient(
      new Map([["/api/runners/model-catalog/", grouped({ codex: [row()], "open-claw": [] })]]),
      calls
    );
    const catalog = await getModelCatalog(client);
    expect(calls[0]?.url).toBe("/api/runners/model-catalog/");
    expect(catalog.agents["codex"]?.[0]?.model).toBe("gpt-5.5");
    expect(catalog.agents["open-claw"]).toEqual([]);
  });

  it("lists the admin catalog from the admin path", async () => {
    const calls: Call[] = [];
    const client = stubClient(
      new Map([["/api/instances/runner-models/", grouped({ codex: [row({ is_enabled: false })] })]]),
      calls
    );
    const catalog = await listAdminRunnerModels(client);
    expect(calls[0]?.url).toBe("/api/instances/runner-models/");
    expect(catalog.agents["codex"]?.[0]?.is_enabled).toBe(false);
  });

  it("posts a create body and parses the created row", async () => {
    const calls: Call[] = [];
    const client = stubClient(
      new Map([["/api/instances/runner-models/", row({ id: "22222222-2222-4222-8222-222222222222" })]]),
      calls
    );
    const created = await createRunnerModel(client, {
      agent: "codex",
      model: "gpt-9-test",
      label: "GPT-9 Test",
      reasoning_efforts: ["low"],
    });
    expect(calls[0]).toEqual({
      method: "POST",
      url: "/api/instances/runner-models/",
      json: {
        agent: "codex",
        model: "gpt-9-test",
        label: "GPT-9 Test",
        reasoning_efforts: ["low"],
      },
    });
    expect(created.id).toBe("22222222-2222-4222-8222-222222222222");
  });

  it("patches a row and answers the status of deletes", async () => {
    const calls: Call[] = [];
    const client = stubClient(
      new Map([[`/api/instances/runner-models/22222222-2222-4222-8222-222222222222/`, row()]]),
      calls
    );
    const updated = await updateRunnerModel(client, "22222222-2222-4222-8222-222222222222", {
      label: "GPT-5.5 Prime",
      is_preselected: true,
    });
    expect(calls[0]?.method).toBe("PATCH");
    expect(updated.label).toBe("GPT-5.5");
    const status = await deleteRunnerModel(client, "22222222-2222-4222-8222-222222222222");
    expect(status).toBe(204);
  });

  it("rejects rows with bad fields", () => {
    expect(() => RunnerModelRow.parse(row({ sort_order: "4" }))).toThrow();
    expect(() => RunnerModelCatalogResponse.parse({ agents: [] })).toThrow();
  });
});
