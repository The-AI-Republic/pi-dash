// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Merged composer picker (NEWPIDASH-394, design section 7.5): the merged
// catalog-plus-BYOK list with the effective choice selected, PUT
// model-selection writes for the picker's own surface, the fallback
// notice, the blocked-commit refusal, failure keeping the old selection,
// and a keyboard gesture that never sends.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const platformMock = vi.hoisted(() => ({ desktop: false }));
vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../core/platform/index.js")>();
  // Read the flag lazily: the factory runs once at import time, but the
  // tests flip the build per case.
  const platform = new Proxy(actual.platform, {
    get: (target, property, receiver) =>
      property === "agentRuntime" && platformMock.desktop ? {} : Reflect.get(target, property, receiver),
  });
  return { ...actual, platform };
});

const notified = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error, notifySuccess: notified.success };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { MODEL_PICKER_BLOCKED_TITLE, MODEL_PICKER_FAILED_TITLE } from "../lib/copy.js";
import { OpenHubModelPicker } from "./OpenHubModelPicker.js";

const BYOK_ID = "223e4567-e89b-12d3-a456-426614174002";

const CATALOG = ["moonshotai/kimi-k3", "deepseek/deepseek-v4-pro", "openhub_free_llm"];

function byokRow(id: string, baseUrl: string, models: string[]) {
  return {
    id,
    kind: "byok" as const,
    provider_kind: "openai_compatible",
    base_url: baseUrl,
    model_name: models[0] ?? "",
    models,
    model_verified_at: {},
    has_api_key: true,
    is_selected: false,
    last_verified_at: null,
    created_at: null,
    connected: true,
    uses_chatgpt_plan: false,
  };
}

let extraRows: ReturnType<typeof byokRow>[] = [];

function connectionsList() {
  return {
    connections: [byokRow(BYOK_ID, "https://llm.example.com/v1", ["gpt-6", "gpt-6.2"]), ...extraRows],
    codex_model_options: [],
    has_codex_credential: false,
    openai_login_available: false,
    openhub_model_options: CATALOG,
    openhub_model_labels: { "moonshotai/kimi-k3": "Kimi K3" },
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
    saved: null,
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "none_saved",
  },
  agent: { saved: null, effective: null, fallback_reason: "lane_unavailable" },
};

const CONNECTIONS = "/api/users/me/ai-assistant/connections/";
const SELECTION = "/api/users/me/ai-assistant/model-selection/";
const QUOTA = "/api/openhub/usage/quota/";

const calls: Array<{ method: string; url: string; body: unknown }> = [];
let putStatus = 200;
let quotaPlan: string | null = null;

function stubTransport(): void {
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
    };
    calls.push(call);
    let status = 200;
    let body: unknown = null;
    if (call.method === "GET" && url.endsWith(CONNECTIONS)) {
      body = connectionsList();
    } else if (call.method === "GET" && url === `https://api.test${SELECTION}`) {
      body = { ...selectionState };
    } else if (
      call.method === "PUT" &&
      (url === `https://api.test${SELECTION}web_assistant/` || url === `https://api.test${SELECTION}desktop_assistant/`)
    ) {
      status = putStatus;
      if (status === 200) {
        const input = call.body as { model: string; connection_id: string | null };
        const surface = url.endsWith("web_assistant/") ? "web_assistant" : "desktop_assistant";
        selectionState[surface] = {
          saved: { model: input.model, connection_id: input.connection_id },
          effective: { model: input.model, connection_id: input.connection_id },
          fallback_reason: null,
        };
        body = { ...selectionState[surface] };
      } else {
        body = { error: "model_not_available", detail: "Model not available." };
      }
    } else if (call.method === "GET" && url.endsWith(QUOTA)) {
      if (quotaPlan === null) {
        status = 500;
        body = { error: "boom" };
      } else {
        body = { llm: { freeTokensUsed: 1, freeTokensLimit: 10, freeModels: ["openhub_free_llm"] }, plan: quotaPlan };
      }
    } else {
      throw new Error(`unexpected call ${call.method} ${url}`);
    }
    return {
      status,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  }) as never);
}

function renderPicker(props: Partial<React.ComponentProps<typeof OpenHubModelPicker>> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const rendered = render(
    <QueryClientProvider client={queryClient}>
      <OpenHubModelPicker workspaceSlug="acme" {...props} />
    </QueryClientProvider>
  );
  return { ...rendered, queryClient };
}

function resetSelection(): void {
  selectionState.web_assistant = {
    saved: { model: "moonshotai/kimi-k3", connection_id: null },
    effective: { model: "moonshotai/kimi-k3", connection_id: null },
    fallback_reason: null,
  };
  selectionState.desktop_assistant = {
    saved: null,
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "none_saved",
  };
  selectionState.agent = { saved: null, effective: null, fallback_reason: "lane_unavailable" };
}

afterEach(() => {
  calls.length = 0;
  extraRows = [];
  putStatus = 200;
  quotaPlan = null;
  platformMock.desktop = false;
  resetSelection();
  notified.error.mockReset();
  resetAppClient();
});

describe("OpenHubModelPicker", () => {
  it("lists catalog models first with labels, then BYOK models, with the effective choice selected", async () => {
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    expect(select).toHaveValue("catalog:moonshotai/kimi-k3");
    const options = screen.getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "Kimi K3",
      "Deepest reasoning, costs more",
      "OpenHub free model (default)",
      "gpt-6 (BYOK · llm.example.com)",
      "gpt-6.2 (BYOK · llm.example.com)",
    ]);
  });

  it("writes a BYOK pick through PUT web_assistant with the connection id", async () => {
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.change(select, { target: { value: `byok:${BYOK_ID}:gpt-6` } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1));
    const put = calls.find((call) => call.method === "PUT");
    expect(put?.url).toBe(`https://api.test${SELECTION}web_assistant/`);
    expect(put?.body).toEqual({ model: "gpt-6", connection_id: BYOK_ID });
    await waitFor(() =>
      expect(calls.filter((call) => call.method === "GET" && call.url === `https://api.test${SELECTION}`)).toHaveLength(
        2
      )
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Assistant model" })).toHaveValue(`byok:${BYOK_ID}:gpt-6`)
    );
  });

  it("writes a catalog pick with a null connection id", async () => {
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.change(select, { target: { value: "catalog:openhub_free_llm" } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1));
    expect(calls.find((call) => call.method === "PUT")?.body).toEqual({
      model: "openhub_free_llm",
      connection_id: null,
    });
  });

  it("writes desktop_assistant in the desktop build", async () => {
    platformMock.desktop = true;
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    expect(select).toHaveValue("catalog:openhub_free_llm");
    fireEvent.change(select, { target: { value: "catalog:moonshotai/kimi-k3" } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1));
    expect(calls.find((call) => call.method === "PUT")?.url).toBe(`https://api.test${SELECTION}desktop_assistant/`);
  });

  it("shows the fallback notice when the saved choice is no longer used", async () => {
    selectionState.web_assistant = {
      saved: { model: "modelC", connection_id: null },
      effective: { model: "openhub_free_llm", connection_id: null },
      fallback_reason: "model_removed",
    };
    stubTransport();
    renderPicker();
    expect(await screen.findByTestId("assistant-model-fallback")).toHaveTextContent(
      "modelC is no longer available. Using OpenHub free model."
    );
    expect(screen.getByRole("combobox", { name: "Assistant model" })).toHaveValue("catalog:openhub_free_llm");
  });

  it("renders for a BYOK-only setup with no catalog models", async () => {
    selectionState.web_assistant = {
      saved: { model: "gpt-6", connection_id: BYOK_ID },
      effective: { model: "gpt-6", connection_id: BYOK_ID },
      fallback_reason: null,
    };
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    expect(select).toHaveValue(`byok:${BYOK_ID}:gpt-6`);
  });

  it("hides when the surface has no effective choice", async () => {
    selectionState.web_assistant = { saved: null, effective: null, fallback_reason: "lane_unavailable" };
    stubTransport();
    renderPicker();
    await waitFor(() => expect(calls.some((call) => call.url === `https://api.test${SELECTION}`)).toBe(true));
    expect(screen.queryByRole("combobox", { name: "Assistant model" })).not.toBeInTheDocument();
  });

  it("keeps the old selection and refetches when the server rejects the pick", async () => {
    putStatus = 500;
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.change(select, { target: { value: "catalog:deepseek/deepseek-v4-pro" } });
    await waitFor(() => expect(notified.error).toHaveBeenCalledWith(MODEL_PICKER_FAILED_TITLE, "Model not available."));
    expect(screen.getByRole("combobox", { name: "Assistant model" })).toHaveValue("catalog:moonshotai/kimi-k3");
    await waitFor(() =>
      expect(calls.filter((call) => call.method === "GET" && call.url === `https://api.test${SELECTION}`)).toHaveLength(
        2
      )
    );
  });

  it("refuses a paid model on the free plan before any request leaves", async () => {
    quotaPlan = "free";
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Deepest reasoning, costs more (Needs credit)" })).toBeDisabled()
    );
    expect(screen.getByRole("option", { name: "OpenHub free model (Free) (default)" })).toBeEnabled();
    fireEvent.change(select, { target: { value: "catalog:deepseek/deepseek-v4-pro" } });
    await waitFor(() => expect(notified.error).toHaveBeenCalledWith(MODEL_PICKER_BLOCKED_TITLE, expect.any(String)));
    expect(calls.some((call) => call.method === "PUT")).toBe(false);
  });

  it("lists the same model ID under two keys as two choices", async () => {
    extraRows = [byokRow("323e4567-e89b-12d3-a456-426614174003", "https://other.example/v2", ["gpt-6"])];
    stubTransport();
    renderPicker();
    await screen.findByRole("combobox", { name: "Assistant model" });
    expect(screen.getByRole("option", { name: "gpt-6 (BYOK · llm.example.com)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "gpt-6 (BYOK · other.example)" })).toBeInTheDocument();
  });

  it("never sends when opened or used with the keyboard", async () => {
    stubTransport();
    const onSubmit = vi.fn((event: React.FormEvent<HTMLFormElement>) => event.preventDefault());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <form onSubmit={onSubmit}>
          <OpenHubModelPicker workspaceSlug="acme" />
        </form>
      </QueryClientProvider>
    );
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.keyDown(select, { key: "Enter" });
    fireEvent.keyDown(select, { key: "Enter", shiftKey: true });
    fireEvent.keyDown(select, { key: "Enter", ctrlKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
    expect(calls.some((call) => call.method === "PUT")).toBe(false);
    expect(select).toHaveValue("catalog:moonshotai/kimi-k3");
  });
});
