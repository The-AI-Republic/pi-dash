// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The OSS Models section (NEWPIDASH-394, design section 7.4): web and
// desktop pickers over the single key's model list — the stored list
// when the backend sends it, else the legacy single model — with a
// null connection on every write. No agent picker in OSS.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { LlmConfig } from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { AssistantModelsSection } from "./AssistantModelsSection.js";

function config(overrides: Partial<LlmConfig> = {}): LlmConfig {
  return {
    provider_kind: "openai_compatible",
    base_url: "https://llm.example.com/v1",
    model_name: "gpt-6",
    has_api_key: true,
    last_verified_at: null,
    models: ["gpt-6", "gpt-6.2"],
    model_verified_at: {},
    ...overrides,
  };
}

interface SurfaceState {
  saved: { model: string; connection_id: string | null } | null;
  effective: { model: string; connection_id: string | null } | null;
  fallback_reason: string | null;
}

const selectionState: Record<"web_assistant" | "desktop_assistant" | "agent", SurfaceState> = {
  web_assistant: {
    saved: { model: "gpt-6", connection_id: null },
    effective: { model: "gpt-6", connection_id: null },
    fallback_reason: null,
  },
  desktop_assistant: {
    saved: null,
    effective: { model: "gpt-6.2", connection_id: null },
    fallback_reason: "none_saved",
  },
  agent: { saved: null, effective: null, fallback_reason: "lane_unavailable" },
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

function renderSection(value: LlmConfig) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <AssistantModelsSection workspaceSlug="acme" config={value} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  calls.length = 0;
  resetAppClient();
});

describe("AssistantModelsSection", () => {
  it("renders web and desktop pickers over the key's list, without an agent picker", async () => {
    stubTransport();
    renderSection(config());
    expect(await screen.findByRole("heading", { name: "Models" })).toBeInTheDocument();
    const web = await screen.findByRole("combobox", { name: "Web assistant" });
    expect(web).toHaveValue("byok:local:gpt-6");
    expect(Array.from(web.querySelectorAll("option")).map((option) => option.textContent)).toEqual([
      "gpt-6 (BYOK · llm.example.com)",
      "gpt-6.2 (BYOK · llm.example.com)",
    ]);
    expect(screen.getByRole("combobox", { name: "Desktop assistant" })).toHaveValue("byok:local:gpt-6.2");
    expect(screen.queryByRole("combobox", { name: "Pi Dash Agent" })).toBeNull();
  });

  it("falls back to the legacy single model without a stored list", async () => {
    stubTransport();
    renderSection(config({ models: undefined }));
    const web = await screen.findByRole("combobox", { name: "Web assistant" });
    expect(Array.from(web.querySelectorAll("option")).map((option) => option.textContent)).toEqual([
      "gpt-6 (BYOK · llm.example.com)",
    ]);
  });

  it("renders nothing when the key holds no IDs", () => {
    stubTransport();
    const { container } = renderSection(config({ models: [], model_name: "" }));
    expect(container).toBeEmptyDOMElement();
    expect(calls).toHaveLength(0);
  });

  it("writes a desktop pick with a null connection", async () => {
    stubTransport();
    renderSection(config());
    const desktop = await screen.findByRole("combobox", { name: "Desktop assistant" });
    fireEvent.change(desktop, { target: { value: "byok:local:gpt-6" } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1));
    const put = calls.find((call) => call.method === "PUT");
    expect(put?.url).toBe(`https://api.test${SELECTION}desktop_assistant/`);
    expect(put?.body).toEqual({ model: "gpt-6", connection_id: null });
  });
});
