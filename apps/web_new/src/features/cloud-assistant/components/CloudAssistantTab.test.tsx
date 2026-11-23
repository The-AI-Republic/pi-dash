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

vi.mock("@tanstack/react-router", () => ({
  useParams: () => ({ ws: "acme" }),
}));

vi.mock("./AssistantModelsSection.js", () => ({
  AssistantModelsSection: ({ workspaceSlug }: { workspaceSlug: string }) => (
    <div data-testid="assistant-models-section-stub">{workspaceSlug}</div>
  ),
}));

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
    expect(await screen.findByText("BYOK:openrouter.ai")).toBeInTheDocument();
    expect(screen.getByText("In use")).toBeInTheDocument();
    expect(screen.getByText("OpenAI account")).toBeInTheDocument();
    expect(screen.queryByRole("radio")).toBeNull();
    expect(screen.getAllByText("2026-02-03T04:05:06Z")[0]?.closest("p")).toHaveTextContent("Verified");
    expect(await screen.findByRole("heading", { name: "Tool servers (MCP)" })).toBeInTheDocument();
    expect(screen.getByTestId("cloud-assistant-apps-pointer")).toHaveTextContent("Looking for apps?");
  });

  it("renders the Models section above the connections with the workspace slug", async () => {
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      return undefined;
    });
    renderTab();
    expect(await screen.findByTestId("assistant-models-section-stub")).toHaveTextContent("acme");
    expect(screen.getByText("BYOK:openrouter.ai")).toBeInTheDocument();
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
    await screen.findByText("BYOK:openrouter.ai");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    expect(screen.queryByRole("heading", { name: "Tool servers (MCP)" })).toBeNull();
    expect(screen.queryByTestId("cloud-assistant-apps-pointer")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Back to connections" }));
    expect(await screen.findByRole("heading", { name: "Tool servers (MCP)" })).toBeInTheDocument();
  });

  it("saves an added model ID with the longer list, and shows a refused write per ID", async () => {
    const user = userEvent.setup();
    let fail: "per-id" | "transport" | null = null;
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "PATCH") {
        if (fail === "per-id") {
          return {
            status: 400,
            body: {
              error: "model_test_failed",
              models: [{ model: "broken-model", status: "failed", detail: "No such model." }],
            },
          };
        }
        if (fail === "transport") return { status: 500, body: { detail: "Server error" } };
        return { status: 200, body: BYOK };
      }
      return undefined;
    });
    renderTab();
    const field = await screen.findByLabelText("Add a model ID this key serves");
    await user.type(field, "other-model{Enter}");
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      models: ["meta-llama/llama-3.3-70b-instruct", "other-model"],
      save_unverified: false,
    });
    expect(notifyError).not.toHaveBeenCalled();
    fail = "per-id";
    await waitFor(() => expect(field).toBeEnabled());
    await user.type(field, "broken-model{Enter}");
    await waitFor(() => expect(screen.getByTestId("byok-model-detail")).toHaveTextContent("No such model."));
    expect(notifyError).not.toHaveBeenCalled();
    fail = "transport";
    await waitFor(() => expect(field).toBeEnabled());
    await user.type(field, "down-model{Enter}");
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Model change failed", "Server error"));
  });

  it("tests one stored model ID and reports its failure inline", async () => {
    const user = userEvent.setup();
    let answer: Record<string, unknown> = { model: "meta-llama/llama-3.3-70b-instruct", status: "verified" };
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "POST" && url.endsWith(`/connections/${BYOK.id}/test/`)) return { status: 200, body: answer };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Test meta-llama/llama-3.3-70b-instruct" }));
    await waitFor(() =>
      expect(calls.some((call) => call.method === "POST" && call.url.endsWith(`/connections/${BYOK.id}/test/`))).toBe(
        true
      )
    );
    expect(notifyError).not.toHaveBeenCalled();
    answer = {
      model: "meta-llama/llama-3.3-70b-instruct",
      status: "failed",
      code: "model_invalid",
      detail: "No such model.",
    };
    await user.click(screen.getByRole("button", { name: "Test meta-llama/llama-3.3-70b-instruct" }));
    await waitFor(() => expect(screen.getByTestId("byok-model-detail")).toHaveTextContent("No such model."));
  });

  it("reports a per-ID test request that never reached the server", async () => {
    const user = userEvent.setup();
    stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET") return { status: 200, body: list() };
      if (method === "POST" && url.endsWith(`/connections/${BYOK.id}/test/`))
        return { status: 500, body: { detail: "Server error" } };
      return undefined;
    });
    renderTab();
    await user.click(await screen.findByRole("button", { name: "Test meta-llama/llama-3.3-70b-instruct" }));
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
    await screen.findByText("BYOK:openrouter.ai");
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(screen.getByRole("button", { name: "Confirm remove" }));
    await waitFor(() => expect(notifyInfo).toHaveBeenCalledWith("Removed", "BYOK:openrouter.ai removed."));
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
    await waitFor(() => expect(screen.queryByText("BYOK:openrouter.ai")).toBeNull());
  });

  it("deletes the connection after confirming the last model ID's removal", async () => {
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
    await user.click(await screen.findByRole("button", { name: "Remove meta-llama/llama-3.3-70b-instruct" }));
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
    expect(screen.getByText("also removes this BYOK:openrouter.ai key", { exact: false })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove both" }));
    await waitFor(() => expect(notifyInfo).toHaveBeenCalledWith("Removed", "BYOK:openrouter.ai removed."));
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
    await waitFor(() => expect(screen.queryByText("BYOK:openrouter.ai")).toBeNull());
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
    await screen.findByText("BYOK:openrouter.ai");
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
    await user.type(
      screen.getByLabelText("Add a model ID this key serves"),
      "meta-llama/llama-3.3-70b-instruct{Enter}"
    );
    await user.type(screen.getByLabelText("API key"), " secret ");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Connection added"));
    expect(calls.find((call) => call.method === "POST" && call.url.endsWith(CONNECTIONS))?.body).toEqual({
      provider_kind: "openai_compatible",
      base_url: "https://openrouter.ai/api/v1",
      model_name: "meta-llama/llama-3.3-70b-instruct",
      models: ["meta-llama/llama-3.3-70b-instruct"],
      api_key: "secret",
    });
    expect(await screen.findByText("BYOK:openrouter.ai")).toBeInTheDocument();
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
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "m{Enter}");
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
    expect(screen.getByText("OpenHub")).toBeInTheDocument();
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
    expect(await screen.findByText("OpenHub")).toBeInTheDocument();
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
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "claude{Enter}");
    await user.type(screen.getByLabelText("API key"), "k");
    await user.click(screen.getByRole("button", { name: "Add connection" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Connection added"));
    expect(calls.find((call) => call.method === "POST" && call.url.endsWith(CONNECTIONS))?.body).toEqual({
      provider_kind: "anthropic",
      model_name: "claude",
      models: ["claude"],
      api_key: "k",
    });
  });

  it("blocks a paid model for a free-plan user before any request leaves", async () => {
    const hosted = {
      ...BYOK,
      kind: "openhub",
      provider_kind: "openhub",
      base_url: "",
      model_name: "openhub_free_llm",
    };
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET" && url.endsWith("/api/openhub/usage/quota/")) {
        return {
          status: 200,
          body: {
            llm: { freeTokensUsed: 1, freeTokensLimit: 10, freeModels: ["openhub_free_llm"] },
            plan: "free",
          },
        };
      }
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            connections: [hosted],
            openhub_model_options: ["openhub_free_llm", "moonshotai/kimi-k3"],
            openhub_wallet_url: "https://hub.example.com/account/credits",
          }),
        };
      }
      if (method === "PATCH") return { status: 200, body: hosted };
      return undefined;
    });
    renderTab();
    const picker = await screen.findByRole("combobox", { name: "Model for OpenHub" });
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Balanced hosted model (Needs credit)" })).toBeInTheDocument()
    );
    expect(screen.getByRole("option", { name: "OpenHub free model (Free)" })).toBeEnabled();
    expect(screen.getByRole("option", { name: "Balanced hosted model (Needs credit)" })).toBeDisabled();
    expect(picker).toHaveValue("openhub_free_llm");
    await waitFor(() => expect(calls.some((call) => call.url.endsWith("/api/openhub/usage/quota/"))).toBe(true));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("refuses a blocked model commit with a toast and sends no PATCH", async () => {
    const { fireEvent } = await import("@testing-library/react");
    const hosted = {
      ...BYOK,
      kind: "openhub",
      provider_kind: "openhub",
      base_url: "",
      model_name: "openhub_free_llm",
    };
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET" && url.endsWith("/api/openhub/usage/quota/")) {
        return {
          status: 200,
          body: { llm: { freeModels: ["openhub_free_llm"] }, plan: "free" },
        };
      }
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            connections: [hosted],
            openhub_model_options: ["openhub_free_llm", "moonshotai/kimi-k3"],
            openhub_wallet_url: "https://hub.example.com/account/credits",
          }),
        };
      }
      if (method === "PATCH") return { status: 200, body: hosted };
      return undefined;
    });
    renderTab();
    const picker = await screen.findByRole("combobox", { name: "Model for OpenHub" });
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Balanced hosted model (Needs credit)" })).toBeInTheDocument()
    );
    // Browsers block picking a disabled option, so drive the change the
    // tab owns directly to prove the second line of defense holds.
    fireEvent.change(picker, { target: { value: "moonshotai/kimi-k3" } });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Model needs credit", expect.anything()));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("lets a free-plan user change a codex model (the meter gates the hosted lane only)", async () => {
    const user = userEvent.setup();
    const hosted = {
      ...BYOK,
      kind: "openhub",
      provider_kind: "openhub",
      base_url: "",
      model_name: "openhub_free_llm",
    };
    const codex = {
      ...CODEX,
      is_selected: false,
      connected: true,
    };
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET" && url.endsWith("/api/openhub/usage/quota/")) {
        return {
          status: 200,
          body: { llm: { freeModels: ["openhub_free_llm"] }, plan: "free" },
        };
      }
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            connections: [hosted, codex],
            openhub_model_options: ["openhub_free_llm", "moonshotai/kimi-k3"],
          }),
        };
      }
      if (method === "PATCH") {
        return {
          status: 200,
          body: { ...codex, model_name: "gpt-5.5" },
        };
      }
      return undefined;
    });
    renderTab();
    // The hosted picker's badge proves the quota answer landed.
    await screen.findByRole("option", { name: "Balanced hosted model (Needs credit)" });
    const picker = screen.getByRole("combobox", { name: "Model for OpenAI account" });
    expect(screen.getByRole("option", { name: "Previous generation" })).toBeEnabled();
    await user.selectOptions(picker, "gpt-5.5");
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")).toMatchObject({
      url: expect.stringContaining("/api/users/me/ai-assistant/connections/"),
      body: { model_name: "gpt-5.5" },
    });
    expect(notifyError).not.toHaveBeenCalledWith("Model needs credit", expect.anything());
  });

  it("lets a free-plan user change a self-supplied model (the meter gates the hosted lane only)", async () => {
    const user = userEvent.setup();
    const hosted = {
      ...BYOK,
      id: "123e4567-e89b-12d3-a456-426614174002",
      kind: "openhub",
      provider_kind: "openhub",
      base_url: "",
      model_name: "openhub_free_llm",
    };
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET" && url.endsWith("/api/openhub/usage/quota/")) {
        return {
          status: 200,
          body: { llm: { freeModels: ["openhub_free_llm"] }, plan: "free" },
        };
      }
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            connections: [hosted, BYOK],
            openhub_model_options: ["openhub_free_llm", "moonshotai/kimi-k3"],
          }),
        };
      }
      if (method === "PATCH") {
        return {
          status: 200,
          body: { ...BYOK, model_name: "other/provider-model" },
        };
      }
      return undefined;
    });
    renderTab();
    // The hosted picker's badge proves the quota answer landed.
    await screen.findByRole("option", { name: "Balanced hosted model (Needs credit)" });
    const field = screen.getByLabelText("Add a model ID this key serves");
    await user.type(field, "other/provider-model{Enter}");
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      models: ["meta-llama/llama-3.3-70b-instruct", "other/provider-model"],
      save_unverified: false,
    });
    expect(notifyError).not.toHaveBeenCalledWith("Model needs credit", expect.anything());
  });

  it("saves a paid model for a paid-plan user and keeps the top-up link hidden", async () => {
    const user = userEvent.setup();
    const hosted = {
      ...BYOK,
      kind: "openhub",
      provider_kind: "openhub",
      base_url: "",
      model_name: "openhub_free_llm",
    };
    const calls = stubTransport((method, url) => {
      if (method === "GET" && url.endsWith(SERVERS)) return { status: 200, body: [] };
      if (method === "GET" && url.endsWith("/api/openhub/usage/quota/")) {
        return {
          status: 200,
          body: { llm: { freeModels: ["openhub_free_llm"] }, plan: "payg" },
        };
      }
      if (method === "GET") {
        return {
          status: 200,
          body: list({
            connections: [hosted],
            openhub_model_options: ["openhub_free_llm", "moonshotai/kimi-k3"],
            openhub_wallet_url: "https://hub.example.com/account/credits",
          }),
        };
      }
      if (method === "PATCH") {
        return {
          status: 200,
          body: { ...hosted, model_name: "moonshotai/kimi-k3" },
        };
      }
      return undefined;
    });
    renderTab();
    const picker = await screen.findByRole("combobox", { name: "Model for OpenHub" });
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "Balanced hosted model (Uses credit)" })).toBeInTheDocument()
    );
    expect(screen.getByRole("option", { name: "Balanced hosted model (Uses credit)" })).toBeEnabled();
    expect(screen.queryByRole("link", { name: "Top up" })).toBeNull();
    await user.selectOptions(picker, "moonshotai/kimi-k3");
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ model_name: "moonshotai/kimi-k3" });
  });
});
