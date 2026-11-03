// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the cloud tab (CLOUD-032/033): the exact request each
// action sends, the refresh after a write, and the failure detail the UI
// shows.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ApiError } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { cloudAssistantKeys } from "./keys.js";
import { actionFailureDetail, cloudAssistantActions, useCloudAssistantActions } from "./mutations.js";

const ID = "123e4567-e89b-12d3-a456-426614174000";

const CONNECTION = {
  id: ID,
  kind: "byok",
  provider_kind: "openai_compatible",
  base_url: "https://openrouter.ai/api/v1",
  model_name: "model",
  has_api_key: true,
  is_selected: true,
  last_verified_at: null,
  created_at: "2026-01-01T00:00:00Z",
};

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function stubTransport(answer: (call: Call) => { status: number; body?: unknown } | undefined): Call[] {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
    };
    calls.push(call);
    const reply = answer(call);
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

afterEach(() => {
  resetAppClient();
});

describe("cloudAssistantActions", () => {
  it("creates a connection with the values it was handed", async () => {
    const calls = stubTransport((call) => (call.method === "POST" ? { status: 201, body: CONNECTION } : undefined));
    await cloudAssistantActions(getAppClient()).create({
      providerKind: "openai_compatible",
      baseUrl: "https://openrouter.ai/api/v1",
      modelName: "model",
      apiKey: "secret",
    });
    expect(calls.at(-1)).toEqual({
      method: "POST",
      url: "https://api.test/api/users/me/ai-assistant/connections/",
      body: {
        provider_kind: "openai_compatible",
        base_url: "https://openrouter.ai/api/v1",
        model_name: "model",
        api_key: "secret",
      },
    });
  });

  it("selects, renames, disconnects and removes through their own endpoints", async () => {
    const calls = stubTransport((call) => {
      if (call.method === "POST" && call.url.endsWith("/select/")) return { status: 200, body: CONNECTION };
      if (call.method === "PATCH") return { status: 200, body: CONNECTION };
      if (call.method === "POST" && call.url.endsWith("/disconnect/")) return { status: 204 };
      if (call.method === "DELETE") return { status: 204 };
      return undefined;
    });
    const actions = cloudAssistantActions(getAppClient());
    await actions.select(ID);
    await actions.setModel(ID, "next-model");
    await actions.disconnect(ID);
    await actions.remove(ID);
    expect(
      calls.filter((call) => !call.url.includes("/auth/get-csrf-token/")).map((call) => `${call.method} ${call.url}`)
    ).toEqual([
      `POST https://api.test/api/users/me/ai-assistant/connections/${ID}/select/`,
      `PATCH https://api.test/api/users/me/ai-assistant/connections/${ID}/`,
      `POST https://api.test/api/users/me/ai-assistant/connections/${ID}/disconnect/`,
      `DELETE https://api.test/api/users/me/ai-assistant/connections/${ID}/`,
    ]);
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ model_name: "next-model" });
  });

  it("tests the active config and starts the provider login", async () => {
    const calls = stubTransport((call) => {
      if (call.url.endsWith("/config/test/"))
        return { status: 200, body: { ok: false, error_code: "provider_unreachable" } };
      if (call.url.endsWith("/openai-login/")) {
        return { status: 200, body: { authorize_url: "https://auth.openai.com/x", state: "state-1" } };
      }
      return undefined;
    });
    const actions = cloudAssistantActions(getAppClient());
    await expect(actions.test()).resolves.toEqual({ ok: false, error_code: "provider_unreachable" });
    await expect(actions.startSignIn()).resolves.toEqual({
      authorize_url: "https://auth.openai.com/x",
      state: "state-1",
    });
    expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
      "GET https://api.test/auth/get-csrf-token/",
      "POST https://api.test/api/users/me/ai-assistant/config/test/",
      "POST https://api.test/api/users/me/ai-assistant/openai-login/",
    ]);
  });

  it("polls the login status with its state", async () => {
    stubTransport((call) =>
      call.url.includes("/api/users/me/ai-assistant/openai-login/status/")
        ? { status: 200, body: { status: "pending" } }
        : undefined
    );
    await expect(cloudAssistantActions(getAppClient()).signInStatus("state-1")).resolves.toEqual({ status: "pending" });
  });
});

describe("useCloudAssistantActions", () => {
  it("refreshes the connections and the config after a write", async () => {
    stubTransport((call) => (call.method === "DELETE" ? { status: 204 } : undefined));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    queryClient.setQueryData(cloudAssistantKeys.connections(), { connections: [] });
    queryClient.setQueryData(cloudAssistantKeys.config(), { model_name: "" });
    const wrapper = ({ children }: { children?: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
    );
    const { result } = renderHook(() => useCloudAssistantActions(), { wrapper });
    await act(async () => {
      await result.current.remove(ID);
      await result.current.invalidate();
    });
    await waitFor(() => expect(queryClient.getQueryState(cloudAssistantKeys.connections())?.isInvalidated).toBe(true));
    expect(queryClient.getQueryState(cloudAssistantKeys.config())?.isInvalidated).toBe(true);
  });
});

describe("actionFailureDetail", () => {
  it("prefers the server message", () => {
    expect(
      actionFailureDetail(new ApiError({ status: 400, code: "bad", message: "Base URL is unreachable" }), "fallback")
    ).toBe("Base URL is unreachable");
  });

  it("falls back to the first field message when the body carries none", () => {
    expect(
      actionFailureDetail(
        new ApiError({ status: 400, code: "bad", message: "", fields: { base_url: [""] } }),
        "fallback"
      )
    ).toBe("fallback");
    expect(
      actionFailureDetail(
        new ApiError({ status: 400, code: "bad", message: "", fields: { base_url: ["Not a URL"] } }),
        "fallback"
      )
    ).toBe("Not a URL");
  });

  it("falls back when the body has neither a message nor an empty field message", () => {
    expect(actionFailureDetail(new ApiError({ status: 400, code: "bad", message: "" }), "fallback")).toBe("fallback");
    expect(
      actionFailureDetail(
        new ApiError({ status: 400, code: "bad", message: "", fields: { base_url: [""] } }),
        "fallback"
      )
    ).toBe("fallback");
    expect(
      actionFailureDetail(new ApiError({ status: 400, code: "bad", message: "", fields: { base_url: [] } }), "fallback")
    ).toBe("fallback");
  });

  it("falls back for anything that is not an API error", () => {
    expect(actionFailureDetail(new Error("boom"), "fallback")).toBe("fallback");
    expect(actionFailureDetail("nope", "fallback")).toBe("fallback");
  });
});
