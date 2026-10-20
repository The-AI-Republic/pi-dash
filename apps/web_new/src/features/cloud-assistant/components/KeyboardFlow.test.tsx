// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Keyboard-only walks through the cloud assistant (parity/specs/cloud.md
// §8, CLOUD-039). Every action in this area is an ordinary control:
// Tab reaches it, Enter fires it, a text field commits on Enter, and a
// confirmation is a plain button group. Nothing here registers a
// shortcut, so the walks below never press an app-specific chord.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { CloudAssistantTab } from "./CloudAssistantTab.js";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import * as React from "react";
import { afterEach } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const BYOK = {
  id: "123e4567-e89b-12d3-a456-426614174001",
  kind: "byok",
  provider_kind: "openai_compatible",
  base_url: "https://openrouter.ai/api/v1",
  model_name: "meta-llama/llama-3.3-70b-instruct",
  has_api_key: true,
  is_selected: true,
  last_verified_at: null,
  created_at: "2026-01-01T00:00:00Z",
  connected: true,
  uses_chatgpt_plan: false,
};

const CONNECTIONS = "/api/users/me/ai-assistant/connections/";

function renderTab(answer: (method: string, path: string) => { status: number; body?: unknown } | undefined) {
  const writes: Array<{ method: string; path: string; body: unknown }> = [];
  createAppClient("https://api.test", async (url, init) => {
    const path = new URL(url).pathname;
    const method = init.method ?? "GET";
    const reply = answer(method, path);
    if (reply === undefined) throw new Error(`unexpected ${method} ${path}`);
    if (method !== "GET") {
      writes.push({
        method,
        path,
        body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
      });
    }
    return {
      status: reply.status,
      url,
      headers: { get: () => null },
      text: async () => (reply.body === undefined ? "" : JSON.stringify(reply.body)),
    };
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <CloudAssistantTab />
    </QueryClientProvider>
  );
  return { writes };
}

afterEach(() => resetAppClient());

describe("cloud assistant keyboard flow", () => {
  it("adds a connection with Tab and Enter alone", async () => {
    const user = userEvent.setup();
    const { writes } = renderTab((method, path) => {
      if (method === "GET" && path === CONNECTIONS) {
        return {
          status: 200,
          body: {
            connections: [],
            codex_model_options: [],
            codex_model_labels: {},
            has_codex_credential: false,
            openai_login_available: true,
            openhub_model_options: [],
            openhub_wallet_url: "",
          },
        };
      }
      if (method === "POST" && path === CONNECTIONS) return { status: 201, body: {} };
      if (path.startsWith("/api/users/me/ai-assistant/mcp-servers/")) return { status: 200, body: [] };
      return undefined;
    });

    // The list's add action is a button reached by Tab and fired by Enter.
    const add = await screen.findByRole("button", { name: "Add connection" });
    add.focus();
    expect(add).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("form", { name: "Add connection" })).toBeInTheDocument();

    // Each field is a labelled control in document order; type, then Enter
    // on the submit button — no custom chord anywhere.
    screen.getByRole("textbox", { name: "Base URL" }).focus();
    await user.keyboard("https://openrouter.ai/api/v1");
    screen.getByRole("combobox", { name: "Model" }).focus();
    await user.keyboard("meta-llama/llama-3.3-70b-instruct");
    screen.getByLabelText("API key").focus();
    await user.keyboard("parity-440-keys-key{TAB}");
    // Tab from the secret lands on the submit button, enabled by now.
    const save = screen.getByRole("button", { name: "Add connection" });
    expect(save).toHaveFocus();
    expect(save).toBeEnabled();
    await user.keyboard("{Enter}");

    expect(writes).toEqual([
      {
        method: "POST",
        path: CONNECTIONS,
        body: {
          provider_kind: "openai_compatible",
          base_url: "https://openrouter.ai/api/v1",
          model_name: "meta-llama/llama-3.3-70b-instruct",
          api_key: "parity-440-keys-key",
        },
      },
    ]);
    expect(await screen.findByRole("button", { name: "Add connection" })).toBeInTheDocument();
  });

  it("confirms a removal from the keyboard with plain buttons", async () => {
    const user = userEvent.setup();
    renderTab((method, path) => {
      if (method === "GET" && path === CONNECTIONS) {
        return {
          status: 200,
          body: {
            connections: [BYOK],
            codex_model_options: [],
            codex_model_labels: {},
            has_codex_credential: false,
            openai_login_available: true,
            openhub_model_options: [],
            openhub_wallet_url: "",
          },
        };
      }
      if (method === "DELETE" && path.includes(`/connections/${BYOK.id}/`)) return { status: 204 };
      if (path.startsWith("/api/users/me/ai-assistant/mcp-servers/")) return { status: 200, body: [] };
      return undefined;
    });
    await screen.findByText("BYOK:openrouter.ai");
    const remove = screen.getByRole("button", { name: "Remove" });
    remove.focus();
    await user.keyboard("{Enter}");
    const confirm = await screen.findByRole("group", { name: "Remove this connection?" });
    const action = screen.getByRole("button", { name: "Confirm remove" });
    expect(confirm).toContainElement(action);
    action.focus();
    await user.keyboard("{Enter}");
    expect(screen.queryByRole("group", { name: "Remove this connection?" })).not.toBeInTheDocument();
  });
});
