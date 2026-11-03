// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cloud tab (CLOUD-032–034, 036): loading, failure, empty and list
// states; each row action and its notice; the add view; provider sign-in;
// and the shared tool-server section staying reachable on the list view.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { notifySuccess, notifyInfo, notifyError } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/toaster.js")>();
  return { ...original, notifySuccess, notifyInfo, notifyError };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CloudAssistantTab } from "./CloudAssistantTab.js";

const BYOK = {
  id: "123e4567-e89b-12d3-a456-426614174001",
  kind: "byok",
  provider_kind: "openai_compatible",
  base_url: "https://openrouter.ai/api/v1",
  model_name: "meta-llama/llama-3.3-70b-instruct",
  has_api_key: true,
  is_selected: true,
  last_verified_at: "2026-02-03T04:05:06Z",
  created_at: "2026-01-01T00:00:00Z",
  connected: true,
  uses_chatgpt_plan: false,
};

const CODEX = {
  ...BYOK,
  id: "123e4567-e89b-12d3-a456-426614174000",
  kind: "codex",
  provider_kind: "codex",
  base_url: "",
  model_name: "gpt-5.6-sol",
  is_selected: false,
  connected: false,
  uses_chatgpt_plan: false,
};

const CONNECTIONS = "/api/users/me/ai-assistant/connections/";
const CONFIG_TEST = "/api/users/me/ai-assistant/config/test/";
const SERVERS = "/api/users/me/ai-assistant/mcp-servers/";

function list(overrides: Record<string, unknown> = {}) {
  return {
    connections: [BYOK, CODEX],
    codex_model_options: ["gpt-5.6-sol", "gpt-5.5"],
    codex_model_labels: { "gpt-5.5": "Previous generation" },
    has_codex_credential: false,
    openai_login_available: true,
    openhub_model_options: [],
    openhub_wallet_url: "",
    ...overrides,
  };
}

type Reply = { status: number; body?: unknown } | undefined;

function stubTransport(answer: (method: string, url: string, body: unknown) => Reply) {
  const calls: Array<{ method: string; url: string; body: unknown }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
    };
    calls.push(call);
    const reply = answer(call.method, call.url, call.body);
    if (reply === undefined) throw new Error(`unexpected call ${call.method} ${url}`);
    const text = reply.body === undefined ? "" : JSON.stringify(reply.body);
    return {
      status: reply.status,
      url,
      headers: { get: () => (reply.body === undefined ? null : "application/json") },
      text: async () => text,
    };
  }) as never);
  return calls;
}

function renderTab() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <CloudAssistantTab />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("CloudAssistantTab", () => {
  it("lists connections with the shared sections below them", async () => {
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      return undefined;
    });
    renderTab();
    expect(await screen.findByRole("radio", { name: "BYOK:openrouter.ai" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "OpenAI account" })).toBeDisabled();
    expect(screen.getAllByText("2026-02-03T04:05:06Z")[0]?.closest("p")).toHaveTextContent("Verified");
    expect(await screen.findByRole("heading", { name: "Tool servers (MCP)" })).toBeInTheDocument();
    expect(screen.getByTestId("cloud-assistant-apps-pointer")).toHaveTextContent("Looking for apps?");
  });

  it("explains an empty list and points at adding a connection", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections: [] }) };
      return undefined;
    });
    renderTab();
    expect(await screen.findByText("No AI provider connected yet.")).toBeInTheDocument();
    const add = screen.getAllByRole("button", { name: "Add connection" });
    expect(add.length).toBeGreaterThan(1);
    await user.click(add[0]!);
    expect(screen.getByRole("form", { name: "Add connection" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back to connections" }));
    expect(screen.getByText("No AI provider connected yet.")).toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: "Add connection" })[1]!);
    expect(screen.getByRole("form", { name: "Add connection" })).toBeInTheDocument();
  });

  it("reports a connections read failure", async () => {
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 500, body: { detail: "Server error" } };
      return undefined;
    });
    renderTab();
    expect(await screen.findByText("The connections could not be loaded.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back to connections" })).toBeNull();
  });

  it("hides the shared sections while the add form is open", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      return undefined;
    });
    renderTab();
    await screen.findByRole("radio", { name: "BYOK:openrouter.ai" });
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    expect(screen.queryByRole("heading", { name: "Tool servers (MCP)" })).toBeNull();
    expect(screen.queryByTestId("cloud-assistant-apps-pointer")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Back to connections" }));
    expect(await screen.findByRole("heading", { name: "Tool servers (MCP)" })).toBeInTheDocument();
  });

  it("switches the active row and confirms", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections: [BYOK, { ...CODEX, connected: true }] }) };
      if (method === "POST" && url.includes("/select/")) return { status: 200, body: BYOK };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("radio", { name: "OpenAI account" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Switched"));
    expect(calls.some((call) => call.url.includes(`/${CODEX.id}/select/`))).toBe(true);
  });

  it("reports a rejected switch with the server detail", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections: [BYOK, { ...CODEX, connected: true }] }) };
      if (method === "POST" && url.includes("/select/")) return { status: 400, body: { detail: "Not connected" } };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("radio", { name: "OpenAI account" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Switch failed", "Not connected"));
  });

  it("saves a renamed self-supplied model silently and reports a refusal", async () => {
    const user = userEvent.setup();
    let fail = false;
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "PATCH") {
        return fail ? { status: 400, body: { detail: "Unknown model" } } : { status: 200, body: BYOK };
      }
      return undefined;
    });
    renderTab();
    const field = await screen.findByRole("combobox", { name: "Model for BYOK:openrouter.ai" });
    await user.clear(field);
    await user.type(field, "other-model{Enter}");
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ model_name: "other-model" });
    expect(notifyError).not.toHaveBeenCalled();
    fail = true;
    await user.clear(field);
    await user.type(field, "broken-model{Enter}");
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Model change failed", "Unknown model"));
  });

  it("tests the active connection and reports both outcomes", async () => {
    const user = userEvent.setup();
    let answer: Record<string, unknown> = { ok: true };
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "POST" && url.endsWith(CONFIG_TEST)) return { status: 200, body: answer };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Test" }));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(
        "Connection works",
        "The provider accepted a request with this connection."
      )
    );
    answer = { ok: false, detail: "401 from provider" };
    await user.click(screen.getByRole("button", { name: "Test" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Connection failed", "401 from provider"));
  });

  it("falls back to the error code, then the default, when a failed test has no detail", async () => {
    const user = userEvent.setup();
    let answer: Record<string, unknown> = { ok: false, error_code: "provider_unreachable" };
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "POST" && url.endsWith(CONFIG_TEST)) return { status: 200, body: answer };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Test" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Connection failed", "provider_unreachable"));
    answer = { ok: false };
    await user.click(screen.getByRole("button", { name: "Test" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Connection failed", "Unexpected response"));
  });

  it("reports a test request that never reached the server", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "POST" && url.endsWith(CONFIG_TEST)) return { status: 500, body: { detail: "Server error" } };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Test" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Connection failed", "Server error"));
  });

  it("removes a self-supplied row after an inline confirmation", async () => {
    const user = userEvent.setup();
    let connections = [BYOK, CODEX];
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections }) };
      if (method === "DELETE" && url.includes(BYOK.id)) {
        connections = [CODEX];
        return { status: 204 };
      }
      return undefined;
    });
    renderTab();
    await screen.findByRole("radio", { name: "BYOK:openrouter.ai" });
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(screen.getByRole("button", { name: "Confirm remove" }));
    await waitFor(() => expect(notifyInfo).toHaveBeenCalledWith("Removed", "BYOK:openrouter.ai removed."));
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
    await waitFor(() => expect(screen.queryByRole("radio", { name: "BYOK:openrouter.ai" })).toBeNull());
  });

  it("reports a failed removal", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "DELETE") return { status: 500, body: { detail: "Server error" } };
      return undefined;
    });
    renderTab();
    await screen.findByRole("radio", { name: "BYOK:openrouter.ai" });
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(screen.getByRole("button", { name: "Confirm remove" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Could not remove", "Server error"));
  });

  it("disconnects the provider account and reports a refusal", async () => {
    const user = userEvent.setup();
    let fail = false;
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections: [{ ...CODEX, connected: true }] }) };
      if (method === "POST" && url.includes("/disconnect/")) {
        return fail ? { status: 500, body: { detail: "Server error" } } : { status: 204 };
      }
      return undefined;
    });
    renderTab();
    const disconnect = await screen.findByRole("button", { name: "Disconnect" });
    await user.click(disconnect);
    await waitFor(() =>
      expect(notifyInfo).toHaveBeenCalledWith(
        "OpenAI account disconnected",
        "The stored credential was removed. Connect the account again whenever you want to use it."
      )
    );
    fail = true;
    await user.click(screen.getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Disconnect failed", "Server error"));
  });

  it("saves the key form, trims it, and returns to the list", async () => {
    const user = userEvent.setup();
    let connections: unknown[] = [CODEX];
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections }) };
      if (method === "POST" && url.endsWith(CONNECTIONS)) {
        connections = [BYOK, CODEX];
        return { status: 201, body: BYOK };
      }
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Add connection" }));
    await user.type(screen.getByRole("textbox", { name: "Base URL" }), " https://openrouter.ai/api/v1 ");
    await user.type(screen.getByRole("combobox", { name: "Model" }), " meta-llama/llama-3.3-70b-instruct ");
    await user.type(screen.getByLabelText("API key"), " secret ");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Connection added"));
    expect(calls.find((call) => call.method === "POST" && call.url.endsWith(CONNECTIONS))?.body).toEqual({
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      api_key: "secret",
    });
    expect(await screen.findByRole("radio", { name: "BYOK:openrouter.ai" })).toBeChecked();
    expect(screen.queryByRole("form", { name: "Add connection" })).toBeNull();
  });

  it("keeps the form and the server's refusal in view after a failed save", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections: [] }) };
      if (method === "POST" && url.endsWith(CONNECTIONS))
        return { status: 400, body: { detail: "Base URL is unreachable" } };
      return undefined;
    });
    renderTab();
    await screen.findByText("No AI provider connected yet.");
    await user.click(screen.getAllByRole("button", { name: "Add connection" })[0]!);
    await user.type(screen.getByRole("textbox", { name: "Base URL" }), "https://bad.test");
    await user.type(screen.getByRole("combobox", { name: "Model" }), "m");
    await user.type(screen.getByLabelText("API key"), "k");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Could not add connection", "Base URL is unreachable")
    );
    expect(screen.getByRole("form", { name: "Add connection" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not add connection: Base URL is unreachable");
  });

  it("offers provider sign-in only when the deployment supports it", async () => {
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ openai_login_available: false }) };
      return undefined;
    });
    renderTab();
    expect(await screen.findByText("OpenAI sign-in is switched off for this deployment.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Continue with ChatGPT" })).toBeNull();
  });

  it("starts provider sign-in from the disconnected account row", async () => {
    const user = userEvent.setup();
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Continue with ChatGPT" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        "Popup blocked",
        "The sign-in window could not open. Allow pop-ups for this site and try again."
      )
    );
    expect(open).toHaveBeenCalledTimes(1);
    open.mockRestore();
  });

  it("keeps the stale notice on a hosted row a stale session cannot reach", async () => {
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            openhub_available: false,
            connections: [
              { ...BYOK, kind: "openhub", provider_kind: "openhub", base_url: "", model_name: "moonshotai/kimi-k3" },
            ],
          }),
        };
      }
      return undefined;
    });
    renderTab();
    expect(await screen.findByText(/predates OpenHub access/)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "OpenHub" })).toBeDisabled();
    expect(screen.queryByRole("combobox", { name: "Model for OpenHub" })).toBeNull();
  });

  it("keeps the hosted row on a gated-off deployment when the server did send it", async () => {
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            openhub_available: false,
            connections: [
              { ...BYOK, kind: "openhub", provider_kind: "openhub", base_url: "", model_name: "moonshotai/kimi-k3" },
            ],
          }),
        };
      }
      return undefined;
    });
    renderTab();
    expect(await screen.findByRole("radio", { name: "OpenHub" })).toBeInTheDocument();
  });

  it("saves a single-vendor key without an address field", async () => {
    const user = userEvent.setup();
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list({ connections: [] }) };
      if (method === "POST" && url.endsWith(CONNECTIONS)) return { status: 201, body: BYOK };
      return undefined;
    });
    renderTab();
    await screen.findByText("No AI provider connected yet.");
    await user.click(screen.getAllByRole("button", { name: "Add connection" })[0]!);
    await user.selectOptions(screen.getByRole("combobox", { name: "Provider" }), "anthropic");
    await user.type(screen.getByRole("combobox", { name: "Model" }), "claude");
    await user.type(screen.getByLabelText("API key"), "k");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Connection added"));
    expect(calls.find((call) => call.method === "POST" && call.url.endsWith(CONNECTIONS))?.body).toEqual({
      provider_kind: "anthropic",
      model_name: "claude",
      api_key: "k",
    });
  });
});
