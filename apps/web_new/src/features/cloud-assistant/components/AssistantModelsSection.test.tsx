// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cloud Models section (NEWPIDASH-394, design section 7.4): three
// surface pickers over the merged catalog-plus-BYOK list, the agent
// limited to catalog models, with the fallback notice per surface.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notified = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error };
});

import type { AssistantConnectionList } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { AssistantModelsSection } from "./AssistantModelsSection.js";

const BYOK_ID = "223e4567-e89b-12d3-a456-426614174002";

function list(): AssistantConnectionList {
  return {
    connections: [
      {
        id: BYOK_ID,
        kind: "byok",
        provider_kind: "openai_compatible",
        base_url: "https://llm.example.com/v1",
        model_name: "gpt-6",
        models: ["gpt-6", "gpt-6.2"],
        model_verified_at: {},
        has_api_key: true,
        is_selected: true,
        last_verified_at: null,
        created_at: "2026-01-01T00:00:00Z",
        connected: true,
        uses_chatgpt_plan: false,
      },
    ],
    codex_model_options: [],
    codex_model_labels: {},
    has_codex_credential: false,
    openai_login_available: false,
    openhub_model_options: ["moonshotai/kimi-k3", "openhub_free_llm"],
    openhub_model_labels: { "moonshotai/kimi-k3": "Kimi K3" },
    openhub_available: true,
    openhub_wallet_url: "",
  };
}

interface SurfaceState {
  saved: { model: string; connection_id: string | null } | null;
  effective: { model: string; connection_id: string | null } | null;
  fallback_reason: string | null;
}

const selectionState: Record<"web_assistant" | "desktop_assistant" | "agent", SurfaceState> = {
  web_assistant: {
    saved: { model: "moonshotai/kimi-k3", connection_id: null },
    effective: { model: "moonshotai/kimi-k3", connection_id: null },
    fallback_reason: null,
  },
  desktop_assistant: {
    saved: { model: "modelC", connection_id: null },
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "model_removed",
  },
  agent: {
    saved: null,
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "none_saved",
  },
};

const SELECTION = "/api/users/me/ai-assistant/model-selection/";

const calls: Array<{ method: string; url: string; body: unknown }> = [];

function stubTransport(): void {
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
    };
    calls.push(call);
    let body: unknown = null;
    if (call.method === "GET" && url === `https://api.test${SELECTION}`) {
      body = { ...selectionState };
    } else if (call.method === "PUT" && url.startsWith(`https://api.test${SELECTION}`)) {
      const input = call.body as { model: string; connection_id: string | null };
      const surface = url.slice(`https://api.test${SELECTION}`.length, -1) as keyof typeof selectionState;
      selectionState[surface] = {
        saved: { model: input.model, connection_id: input.connection_id },
        effective: { model: input.model, connection_id: input.connection_id },
        fallback_reason: null,
      };
      body = { ...selectionState[surface] };
    } else {
      throw new Error(`unexpected call ${call.method} ${url}`);
    }
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  }) as never);
}

function renderSection() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <AssistantModelsSection workspaceSlug="acme" list={list()} quota={null} />
    </QueryClientProvider>
  );
}

function resetSelection(): void {
  selectionState.web_assistant = {
    saved: { model: "moonshotai/kimi-k3", connection_id: null },
    effective: { model: "moonshotai/kimi-k3", connection_id: null },
    fallback_reason: null,
  };
  selectionState.desktop_assistant = {
    saved: { model: "modelC", connection_id: null },
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "model_removed",
  };
  selectionState.agent = {
    saved: null,
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "none_saved",
  };
}

afterEach(() => {
  calls.length = 0;
  resetSelection();
  notified.error.mockReset();
  resetAppClient();
});

describe("AssistantModelsSection", () => {
  it("renders three surface pickers with their effective choices and the fallback notice", async () => {
    stubTransport();
    renderSection();
    expect(await screen.findByRole("heading", { name: "Models" })).toBeInTheDocument();
    expect(await screen.findByRole("combobox", { name: "Web assistant" })).toHaveValue("catalog:moonshotai/kimi-k3");
    expect(screen.getByRole("combobox", { name: "Desktop assistant" })).toHaveValue("catalog:openhub_free_llm");
    expect(screen.getByRole("combobox", { name: "Pi Dash Agent" })).toHaveValue("catalog:openhub_free_llm");
    expect(screen.getByTestId("surface-model-fallback-desktop_assistant")).toHaveTextContent(
      "modelC is no longer available. Using OpenHub free model."
    );
    expect(screen.queryByTestId("surface-model-fallback-web_assistant")).toBeNull();
  });

  it("offers BYOK models on the assistant surfaces and catalog models only on the agent", async () => {
    stubTransport();
    renderSection();
    const web = await screen.findByRole("combobox", { name: "Web assistant" });
    expect(Array.from(web.querySelectorAll("option")).map((option) => option.textContent)).toEqual([
      "Kimi K3",
      "OpenHub free model (default)",
      "gpt-6 (BYOK · llm.example.com)",
      "gpt-6.2 (BYOK · llm.example.com)",
    ]);
    const agent = screen.getByRole("combobox", { name: "Pi Dash Agent" });
    expect(Array.from(agent.querySelectorAll("option")).map((option) => option.textContent)).toEqual([
      "Kimi K3",
      "OpenHub free model (default)",
    ]);
  });

  it("writes an agent pick through PUT agent only", async () => {
    stubTransport();
    renderSection();
    const agent = await screen.findByRole("combobox", { name: "Pi Dash Agent" });
    fireEvent.change(agent, { target: { value: "catalog:moonshotai/kimi-k3" } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1));
    const put = calls.find((call) => call.method === "PUT");
    expect(put?.url).toBe(`https://api.test${SELECTION}agent/`);
    expect(put?.body).toEqual({ model: "moonshotai/kimi-k3", connection_id: null });
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Pi Dash Agent" })).toHaveValue("catalog:moonshotai/kimi-k3")
    );
  });
});
