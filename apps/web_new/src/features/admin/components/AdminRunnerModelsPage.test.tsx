// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner model catalog page: header, loading skeleton, per-agent
// sections with tables, the add/edit dialog, toggle, preselect,
// reorder, delete, and the load-failure retry. Reads run through the
// real app client against a stub transport so the request surface is
// pinned here.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { RUNNER_MODELS_DELETE_NOTE, RUNNER_MODELS_DESCRIPTION, RUNNER_MODELS_HEADING } from "../runnerModels.js";
import { AdminRunnerModelsPage } from "./AdminRunnerModelsPage.js";

interface ModelRow {
  id: string;
  agent: string;
  model: string;
  label: string;
  reasoning_efforts: string[];
  sort_order: number;
  is_enabled: boolean;
  is_preselected: boolean;
  created_at: string;
  updated_at: string;
}

function modelRow(overrides: Partial<ModelRow>): ModelRow {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    agent: "codex",
    model: "gpt-5.5",
    label: "GPT-5.5",
    reasoning_efforts: ["low", "high"],
    sort_order: 0,
    is_enabled: true,
    is_preselected: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

interface ModelsHarness {
  requests: Array<{ method: string; url: string; body: unknown }>;
  allowList: () => void;
  failList: () => void;
}

async function renderPage(options: {
  agents?: Record<string, ModelRow[]>;
  listFails?: boolean;
  listGate?: Promise<void>;
  expectReady?: boolean;
}): Promise<ModelsHarness> {
  const requests: Array<{ method: string; url: string; body: unknown }> = [];
  let listFails = options.listFails ?? false;
  const listGate = options.listGate ?? Promise.resolve();
  const stored: Record<string, ModelRow[]> = { ...(options.agents ?? {}) };
  const transport: Transport = async (url, init) => {
    const parsed = new URL(url, "https://api.test");
    const path = parsed.pathname;
    const method = init.method;
    if (path === "/api/instances/runner-models/") {
      if (method === "GET") {
        requests.push({ method, url, body: undefined });
        await listGate;
        if (listFails) {
          return {
            status: 500,
            url,
            headers: { get: () => null },
            text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
          };
        }
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ agents: stored })),
        };
      }
      if (method === "POST") {
        const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
        requests.push({ method, url, body });
        const created = modelRow({
          id: "22222222-2222-4222-8222-222222222222",
          agent: String(body["agent"]),
          model: String(body["model"]),
          label: String(body["label"]),
          reasoning_efforts: (body["reasoning_efforts"] as string[] | undefined) ?? [],
          sort_order: Number(body["sort_order"] ?? 0),
          is_enabled: Boolean(body["is_enabled"] ?? true),
          is_preselected: Boolean(body["is_preselected"] ?? false),
        });
        const agent = String(body["agent"]);
        stored[agent] = [...(stored[agent] ?? []), created];
        return {
          status: 201,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify(created)),
        };
      }
    }
    const detail = path.match(/^\/api\/instances\/runner-models\/(.+)\/$/);
    if (detail !== null) {
      const id = detail[1] ?? "";
      if (method === "PATCH") {
        const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
        requests.push({ method, url, body });
        for (const rows of Object.values(stored)) {
          const found = rows.find((entry) => entry.id === id);
          if (found !== undefined) {
            if (typeof body["label"] === "string") found.label = body["label"];
            if (typeof body["model"] === "string") found.model = body["model"];
            if (Array.isArray(body["reasoning_efforts"]))
              found.reasoning_efforts = body["reasoning_efforts"] as string[];
            if (typeof body["sort_order"] === "number") found.sort_order = body["sort_order"];
            if (typeof body["is_enabled"] === "boolean") found.is_enabled = body["is_enabled"];
            if (typeof body["is_preselected"] === "boolean") found.is_preselected = body["is_preselected"];
            if (found.is_preselected) {
              for (const other of rows) {
                if (other.id !== id) other.is_preselected = false;
              }
            }
            return {
              status: 200,
              url,
              headers: { get: () => null },
              text: () => Promise.resolve(JSON.stringify(found)),
            };
          }
        }
        return {
          status: 404,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ error: "not_found" })),
        };
      }
      if (method === "DELETE") {
        requests.push({ method, url, body: undefined });
        for (const [agent, rows] of Object.entries(stored)) {
          stored[agent] = rows.filter((entry) => entry.id !== id);
        }
        return { status: 204, url, headers: { get: () => null }, text: () => Promise.resolve("") };
      }
    }
    throw new Error(`unexpected request ${method} ${url}`);
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AdminRunnerModelsPage />
    </QueryClientProvider>
  );
  await screen.findByTestId("runner-models-page");
  if (options.expectReady !== false) {
    await waitFor(() => {
      const settled =
        screen.queryByTestId("runner-models-skeleton") === null || screen.queryByTestId("runner-models-error") !== null;
      expect(settled).toBe(true);
    });
  }
  return {
    requests,
    failList: () => {
      listFails = true;
    },
    allowList: () => {
      listFails = false;
    },
  };
}

afterEach(() => {
  resetAppClient();
});

describe("AdminRunnerModelsPage", () => {
  it("shows the header, note and skeleton while loading", async () => {
    let release!: () => void;
    const listGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await renderPage({ agents: { codex: [modelRow({})] }, listGate, expectReady: false });
    expect(screen.getByRole("heading", { name: RUNNER_MODELS_HEADING })).toBeInTheDocument();
    expect(screen.getByText(RUNNER_MODELS_DESCRIPTION)).toBeInTheDocument();
    expect(screen.getByText(RUNNER_MODELS_DELETE_NOTE)).toBeInTheDocument();
    expect(screen.getByTestId("runner-models-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("runner-models-table-codex")).not.toBeInTheDocument();
    release();
    await screen.findByTestId("runner-models-table-codex");
  });

  it("renders one section per agent with its table", async () => {
    await renderPage({
      agents: {
        codex: [modelRow({})],
        "claude-code": [
          modelRow({
            id: "33333333-3333-4333-8333-333333333333",
            agent: "claude-code",
            model: "claude-opus-4-8",
            label: "Opus 4.8",
            reasoning_efforts: [],
            is_preselected: true,
          }),
        ],
      },
    });
    await screen.findByTestId("runner-models-table-codex");
    expect(screen.getByTestId("runner-models-section-claude-code")).toBeInTheDocument();
    expect(screen.getByTestId("runner-models-section-open-claw")).toBeInTheDocument();
    expect(screen.getByTestId("runner-models-empty-open-claw")).toHaveTextContent("No models for this agent.");
    expect(screen.getByTestId("runner-models-row-11111111-1111-4111-8111-111111111111")).toHaveTextContent("gpt-5.5");
  });

  it("adds a model through the dialog and refetches", async () => {
    const user = userEvent.setup();
    const harness = await renderPage({ agents: { grok: [] } });
    await user.click(screen.getByTestId("runner-models-add-grok"));
    const dialog = await screen.findByTestId("runner-models-dialog");
    expect(dialog).toBeInTheDocument();
    await user.type(screen.getByTestId("runner-models-model"), "grok-new-1");
    await user.type(screen.getByTestId("runner-models-label"), "Grok New 1");
    await user.click(screen.getByTestId("runner-models-save"));
    await waitFor(() => {
      expect(harness.requests.some((request) => request.method === "POST")).toBe(true);
    });
    const post = harness.requests.find((request) => request.method === "POST");
    expect(post?.body).toMatchObject({ agent: "grok", model: "grok-new-1", label: "Grok New 1" });
  });

  it("edits a model, toggles it, preselects and deletes it", async () => {
    const user = userEvent.setup();
    const id = "11111111-1111-4111-8111-111111111111";
    const harness = await renderPage({
      agents: {
        codex: [
          modelRow({ id, model: "gpt-5.5", label: "GPT-5.5" }),
          modelRow({
            id: "44444444-4444-4444-8444-444444444444",
            model: "gpt-5.4",
            label: "GPT-5.4",
            sort_order: 1,
          }),
        ],
      },
    });
    await user.click(screen.getByTestId(`runner-models-toggle-${id}`));
    await waitFor(() => {
      expect(harness.requests.some((request) => request.method === "PATCH")).toBe(true);
    });
    await user.click(screen.getByTestId(`runner-models-preselect-${id}`));
    await waitFor(() => {
      expect(harness.requests.filter((request) => request.method === "PATCH")).toHaveLength(2);
    });
    await user.click(screen.getByTestId(`runner-models-edit-${id}`));
    await screen.findByTestId("runner-models-dialog");
    await user.clear(screen.getByTestId("runner-models-label"));
    await user.type(screen.getByTestId("runner-models-label"), "GPT-5.5 Prime");
    await user.click(screen.getByTestId("runner-models-save"));
    await waitFor(() => {
      expect(harness.requests.filter((request) => request.method === "PATCH")).toHaveLength(3);
    });
    await user.click(screen.getByTestId(`runner-models-delete-${id}`));
    await user.click(screen.getByTestId(`runner-models-confirm-${id}`));
    await waitFor(() => {
      expect(harness.requests.some((request) => request.method === "DELETE")).toBe(true);
    });
  });

  it("moves a row up and down", async () => {
    const user = userEvent.setup();
    await renderPage({
      agents: {
        grok: [
          modelRow({
            id: "11111111-1111-4111-8111-111111111111",
            agent: "grok",
            model: "grok-a",
            label: "A",
            sort_order: 0,
          }),
          modelRow({
            id: "22222222-2222-4222-8222-222222222222",
            agent: "grok",
            model: "grok-b",
            label: "B",
            sort_order: 1,
          }),
        ],
      },
    });
    expect(screen.getByTestId("runner-models-up-11111111-1111-4111-8111-111111111111")).toBeDisabled();
    expect(screen.getByTestId("runner-models-down-22222222-2222-4222-8222-222222222222")).toBeDisabled();
    await user.click(screen.getByTestId("runner-models-down-11111111-1111-4111-8111-111111111111"));
    await waitFor(() => {
      expect(screen.queryByTestId("runner-models-dialog")).not.toBeInTheDocument();
    });
  });

  it("shows the error block with a retry", async () => {
    const harness = await renderPage({ agents: { grok: [] }, listFails: true });
    const error = await screen.findByTestId("runner-models-error");
    expect(error).toBeInTheDocument();
    harness.allowList();
    await userEvent.setup().click(screen.getByRole("button", { name: "Retry" }));
    await screen.findByTestId("runner-models-table-grok");
  });
});
