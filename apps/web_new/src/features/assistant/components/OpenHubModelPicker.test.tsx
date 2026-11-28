// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// OpenHub composer picker (NEWPIDASH-303): god-mode order with the
// current model selected, PATCH write with list/config refresh, hidden
// lanes, blocked-commit refusal, failure keeps the old selection, and a
// keyboard gesture that never sends.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { LlmConfig } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notified = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error, notifySuccess: notified.success };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { MODEL_PICKER_BLOCKED_TITLE, MODEL_PICKER_FAILED_TITLE } from "../lib/copy.js";
import { OpenHubModelPicker } from "./OpenHubModelPicker.js";

const ROW_ID = "123e4567-e89b-12d3-a456-426614174001";
const CONFIG: LlmConfig = {
  provider_kind: "openhub",
  base_url: "",
  model_name: "moonshotai/kimi-k3",
  has_api_key: true,
  last_verified_at: null,
  credential_mode: "openhub",
  has_openhub_access: true,
};

const CONNECTIONS = "/api/users/me/ai-assistant/connections/";
const CLOUD_CONFIG = "/api/users/me/ai-assistant/config/";
const QUOTA = "/api/openhub/usage/quota/";

function row(model: string, selected = true) {
  return {
    id: ROW_ID,
    kind: "openhub",
    provider_kind: "openhub",
    base_url: "",
    model_name: model,
    has_api_key: true,
    is_selected: selected,
    last_verified_at: null,
    created_at: null,
    connected: true,
    uses_chatgpt_plan: false,
  };
}

function list(
  model: string,
  options: string[] = ["moonshotai/kimi-k3", "deepseek/deepseek-v4-pro", "openhub_free_llm"]
) {
  return {
    connections: [row(model, rowSelected)],
    codex_model_options: [],
    has_codex_credential: false,
    openai_login_available: false,
    openhub_model_options: options,
    openhub_wallet_url: "",
  };
}

const calls: Array<{ method: string; url: string; body: unknown }> = [];
let currentModel = "moonshotai/kimi-k3";
let rowSelected = true;
let patchStatus = 200;
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
      body = list(currentModel);
    } else if (call.method === "PATCH" && url.endsWith(`/${ROW_ID}/`)) {
      status = patchStatus;
      body = status === 200 ? row(currentModel) : { error: "unknown model" };
      if (status === 200) {
        const input = call.body as { model_name: string };
        currentModel = input.model_name;
      }
    } else if (call.method === "GET" && url.endsWith(CLOUD_CONFIG)) {
      body = { ...CONFIG, model_name: currentModel };
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
      <OpenHubModelPicker workspaceSlug="acme" config={CONFIG} {...props} />
    </QueryClientProvider>
  );
  return { ...rendered, queryClient };
}

afterEach(() => {
  calls.length = 0;
  currentModel = "moonshotai/kimi-k3";
  rowSelected = true;
  patchStatus = 200;
  quotaPlan = null;
  notified.error.mockReset();
  resetAppClient();
});

describe("OpenHubModelPicker", () => {
  it("lists the god-mode models in order with the current model selected and the default marked", async () => {
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    expect(screen.getByTestId("assistant-model-picker")).toBe(select);
    expect(select).toHaveValue("moonshotai/kimi-k3");
    const options = screen.getAllByRole("option");
    expect(options.map((option) => option.getAttribute("value"))).toEqual([
      "moonshotai/kimi-k3",
      "deepseek/deepseek-v4-pro",
      "openhub_free_llm",
    ]);
    expect(screen.getByRole("option", { name: "OpenHub free model (default)" })).toBeInTheDocument();
  });

  it("shows an admin-added model after reload with no deploy", async () => {
    stubTransport();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    // Reload answers the extended god-mode list; the picker just renders it.
    createAppClient("https://api.test", (async (url: string) => {
      calls.push({ method: "GET", url, body: null });
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          JSON.stringify(list("moonshotai/kimi-k3", ["moonshotai/kimi-k3", "openhub_free_llm", "brand/new-model"])),
      };
    }) as never);
    render(
      <QueryClientProvider client={queryClient}>
        <OpenHubModelPicker workspaceSlug="acme" config={CONFIG} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: "brand/new-model" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Assistant model" })).toHaveValue("moonshotai/kimi-k3");
  });

  it("writes the pick through PATCH and refreshes the list", async () => {
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.change(select, { target: { value: "deepseek/deepseek-v4-pro" } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PATCH")).toHaveLength(1));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ model_name: "deepseek/deepseek-v4-pro" });
    await waitFor(() =>
      expect(calls.filter((call) => call.method === "GET" && call.url.endsWith(CONNECTIONS))).toHaveLength(2)
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Assistant model" })).toHaveValue("deepseek/deepseek-v4-pro")
    );
  });

  it("keeps the old selection and refetches the list when the server rejects the slug", async () => {
    patchStatus = 500;
    stubTransport();
    renderPicker();
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.change(select, { target: { value: "deepseek/deepseek-v4-pro" } });
    await waitFor(() => expect(notified.error).toHaveBeenCalledWith(MODEL_PICKER_FAILED_TITLE, "unknown model"));
    expect(screen.getByRole("combobox", { name: "Assistant model" })).toHaveValue("moonshotai/kimi-k3");
    await waitFor(() =>
      expect(calls.filter((call) => call.method === "GET" && call.url.endsWith(CONNECTIONS))).toHaveLength(2)
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
    fireEvent.change(select, { target: { value: "deepseek/deepseek-v4-pro" } });
    await waitFor(() => expect(notified.error).toHaveBeenCalledWith(MODEL_PICKER_BLOCKED_TITLE, expect.any(String)));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("never sends when opened or used with the keyboard", async () => {
    stubTransport();
    const onSubmit = vi.fn((event: React.FormEvent<HTMLFormElement>) => event.preventDefault());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <form onSubmit={onSubmit}>
          <OpenHubModelPicker workspaceSlug="acme" config={CONFIG} />
        </form>
      </QueryClientProvider>
    );
    const select = await screen.findByRole("combobox", { name: "Assistant model" });
    fireEvent.keyDown(select, { key: "Enter" });
    fireEvent.keyDown(select, { key: "Enter", shiftKey: true });
    fireEvent.keyDown(select, { key: "Enter", ctrlKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(select).toHaveValue("moonshotai/kimi-k3");
  });
});

describe("OpenHubModelPicker visibility", () => {
  it("hides for a self-supplied lane", () => {
    stubTransport();
    renderPicker({ config: { ...CONFIG, credential_mode: "byok", has_byok_api_key: true } });
    expect(screen.queryByRole("combobox", { name: "Assistant model" })).not.toBeInTheDocument();
  });

  it("hides for a Codex lane", () => {
    stubTransport();
    renderPicker({ config: { ...CONFIG, credential_mode: "codex", has_codex_token: true } });
    expect(screen.queryByRole("combobox", { name: "Assistant model" })).not.toBeInTheDocument();
  });

  it("hides without OpenHub access", () => {
    stubTransport();
    renderPicker({ config: { ...CONFIG, has_openhub_access: false } });
    expect(screen.queryByRole("combobox", { name: "Assistant model" })).not.toBeInTheDocument();
  });

  it("hides when no OpenHub row is selected", async () => {
    rowSelected = false;
    stubTransport();
    renderPicker();
    await waitFor(() => expect(calls.some((call) => call.url.endsWith(CONNECTIONS))).toBe(true));
    await waitFor(() => expect(calls.some((call) => call.url.endsWith(QUOTA))).toBe(true));
    expect(screen.queryByRole("combobox", { name: "Assistant model" })).not.toBeInTheDocument();
  });
});
