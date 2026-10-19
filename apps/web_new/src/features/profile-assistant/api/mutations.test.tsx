// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI-assistant settings hooks (parity/specs/profile.md §10): each request,
// its cached answer, its notice and the failure wording.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport, TransportInit } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notifySuccess, notifyError, notifyInfo } = vi.hoisted(() => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
  notifyInfo: vi.fn(),
}));

vi.mock("../../../shared/shell/toaster.js", () => ({ notifySuccess, notifyError, notifyInfo }));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  CONNECTION_FAILED_TITLE,
  CONNECTION_OK_TITLE,
  LLM_REMOVED_MESSAGE,
  LLM_REMOVED_TITLE,
  LLM_SAVED_MESSAGE,
  LLM_SAVED_TITLE,
  SERVER_ADDED_MESSAGE,
  SERVER_ADDED_TITLE,
  SERVER_REMOVE_FAILED_TITLE,
  SERVER_REMOVED_TITLE,
  STT_REMOVED_MESSAGE,
  STT_SAVED_MESSAGE,
  UNKNOWN_ERROR_FALLBACK,
} from "../lib/copy.js";
import { assistantSettingsKeys } from "./keys.js";
import {
  useAddToolServer,
  useRemoveAssistantLlm,
  useRemoveAssistantVoice,
  useRemoveToolServer,
  useSaveAssistantLlm,
  useSaveAssistantVoice,
  useTestAssistantLlm,
  useTestAssistantVoice,
  useToggleToolServer,
} from "./mutations.js";

interface Call {
  method: string;
  url: string;
  body: unknown;
}

type Reply = { status: number; body?: unknown };

function stubTransport(handler: (url: string, init: TransportInit) => Reply) {
  const calls: Call[] = [];
  const transport: Transport = async (url, init) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "t" }) };
    }
    const parsed = typeof init.body === "string" ? (JSON.parse(init.body) as unknown) : init.body;
    calls.push({ method: init.method, url, body: parsed });
    const reply = handler(url, init);
    return {
      status: reply.status,
      url,
      headers: { get: () => null },
      text: async () => (reply.body === undefined ? "" : JSON.stringify(reply.body)),
    };
  };
  return { transport, calls };
}

function wrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

const LLM = {
  provider_kind: "openai_compatible",
  base_url: "http://127.0.0.1:9",
  model_name: "m",
  has_api_key: true,
  last_verified_at: null,
};

const SERVER_ID = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";
const SERVER = {
  id: SERVER_ID,
  name: "probe",
  url: "https://mcp.test",
  has_auth_header: false,
  tool_prefix: "probe",
  effective_tool_prefix: "probe",
  is_enabled: true,
  created_at: "2026-10-19T00:00:00Z",
  updated_at: "2026-10-19T00:00:00Z",
};

beforeEach(() => {
  notifySuccess.mockReset();
  notifyError.mockReset();
  notifyInfo.mockReset();
});

afterEach(() => resetAppClient());

describe("language-model hooks", () => {
  it("saves the typed config and caches the answer", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: LLM }));
    createAppClient("https://api.test", transport);
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useSaveAssistantLlm(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ provider_kind: "anthropic", model_name: "m", api_key: "key-1" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({ method: "PUT", url: "https://api.test/api/users/me/ai-assistant/config/" });
    expect(calls[0]?.body).toEqual({ provider_kind: "anthropic", model_name: "m", api_key: "key-1" });
    expect(notifySuccess).toHaveBeenCalledWith(LLM_SAVED_TITLE, LLM_SAVED_MESSAGE);
    void client;
  });

  it("reports the server message on a refused save", async () => {
    const { transport } = stubTransport(() => ({ status: 500, body: { error: "probe_boom" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useSaveAssistantLlm(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ model_name: "m", api_key: "key" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Save failed", "probe_boom");
  });

  it("toasts a passing probe and refreshes the config read", async () => {
    const { transport } = stubTransport(() => ({ status: 200, body: { ok: true } }));
    createAppClient("https://api.test", transport);
    const Wrapper = wrapper();
    const client = (Wrapper as unknown as { client?: QueryClient }).client;
    void client;
    const { result } = renderHook(() => useTestAssistantLlm(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(CONNECTION_OK_TITLE, "Your provider responded.");
  });

  it("toasts the probe's failure code", async () => {
    const { transport } = stubTransport(() => ({
      status: 200,
      body: { ok: false, error_code: "provider_unreachable" },
    }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useTestAssistantLlm(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(CONNECTION_FAILED_TITLE, "provider_unreachable");
  });

  it("falls back when the probe answers without a code", async () => {
    const { transport } = stubTransport(() => ({ status: 200, body: { ok: false } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useTestAssistantLlm(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(CONNECTION_FAILED_TITLE, UNKNOWN_ERROR_FALLBACK);
  });

  it("reports a probe request that fails outright", async () => {
    const { transport } = stubTransport(() => ({ status: 429, body: { error: "throttled" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useTestAssistantLlm(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(CONNECTION_FAILED_TITLE, "throttled");
  });

  it("removes the config, notices, and re-reads it", async () => {
    const { transport, calls } = stubTransport((url) =>
      url.endsWith("/config/") ? { status: 204 } : { status: 200, body: LLM }
    );
    createAppClient("https://api.test", transport);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    function Wrapper({ children }: { children?: unknown }): React.ReactElement {
      return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
    }
    const { result } = renderHook(() => useRemoveAssistantLlm(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({ method: "DELETE", url: "https://api.test/api/users/me/ai-assistant/config/" });
    expect(notifyInfo).toHaveBeenCalledWith(LLM_REMOVED_TITLE, LLM_REMOVED_MESSAGE);
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: assistantSettingsKeys.llm }));
  });

  it("reports a refused removal", async () => {
    const { transport } = stubTransport(() => ({ status: 500, body: { detail: "nope" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useRemoveAssistantLlm(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Save failed", "nope");
  });
});

describe("voice hooks", () => {
  it("saves the dictation config", async () => {
    const { transport, calls } = stubTransport(() => ({
      status: 200,
      body: { base_url: "http://127.0.0.1:9", model_name: "whisper-1", has_api_key: true, last_verified_at: null },
    }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useSaveAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ base_url: "http://127.0.0.1:9", model_name: "whisper-1", api_key: "k" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({ method: "PUT", url: "https://api.test/api/users/me/ai-assistant/stt-config/" });
    expect(notifySuccess).toHaveBeenCalledWith(LLM_SAVED_TITLE, STT_SAVED_MESSAGE);
  });

  it("reports a refused dictation save with the fallback line", async () => {
    const { transport } = stubTransport(() => ({ status: 500 }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useSaveAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ base_url: "u", model_name: "m", api_key: "k" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Save failed", "Invalid configuration");
  });

  it("probes the saved dictation endpoint", async () => {
    const { transport } = stubTransport(() => ({ status: 200, body: { ok: true } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useTestAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(CONNECTION_OK_TITLE, "Your transcription endpoint responded.");
  });

  it("reports a failed dictation probe with its code", async () => {
    const { transport } = stubTransport(() => ({ status: 200, body: { ok: false, error_code: "bad_key" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useTestAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(CONNECTION_FAILED_TITLE, "bad_key");
  });

  it("reports a dictation probe request that fails", async () => {
    const { transport } = stubTransport(() => ({ status: 502, body: { error: "upstream" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useTestAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(CONNECTION_FAILED_TITLE, "upstream");
  });

  it("removes the dictation config", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 204 }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useRemoveAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({ method: "DELETE", url: "https://api.test/api/users/me/ai-assistant/stt-config/" });
    expect(notifyInfo).toHaveBeenCalledWith(LLM_REMOVED_TITLE, STT_REMOVED_MESSAGE);
  });

  it("reports a refused dictation removal", async () => {
    const { transport } = stubTransport(() => ({ status: 500, body: { error: "boom" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useRemoveAssistantVoice(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Save failed", "boom");
  });
});

describe("tool-server hooks", () => {
  it("adds a server and notices", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 201, body: SERVER }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useAddToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ name: "probe", url: "https://mcp.test" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({ method: "POST", url: "https://api.test/api/users/me/ai-assistant/mcp-servers/" });
    expect(notifySuccess).toHaveBeenCalledWith(SERVER_ADDED_TITLE, SERVER_ADDED_MESSAGE);
  });

  it("reports a duplicate name with the server's own message", async () => {
    const { transport } = stubTransport(() => ({
      status: 400,
      body: { error: "duplicate_name", detail: "You already have a tool server with that name." },
    }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useAddToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ name: "probe", url: "https://mcp.test" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Could not add server", "You already have a tool server with that name.");
  });

  it("falls back to its own add line when the refusal has no message", async () => {
    const { transport } = stubTransport(() => ({ status: 500 }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useAddToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ name: "probe", url: "https://mcp.test" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Could not add server", "Invalid tool server.");
  });

  it("toggles a server's enabled flag", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 200, body: { ...SERVER, is_enabled: false } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useToggleToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ serverId: SERVER_ID, isEnabled: false });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({
      method: "PATCH",
      url: `https://api.test/api/users/me/ai-assistant/mcp-servers/${SERVER_ID}/`,
      body: { is_enabled: false },
    });
  });

  it("reports a failed toggle", async () => {
    const { transport } = stubTransport(() => ({ status: 500, body: { error: "gone" } }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useToggleToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ serverId: SERVER_ID, isEnabled: true });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Update failed", "gone");
  });

  it("uses the toggle fallback when the refusal carries no message", async () => {
    const { transport } = stubTransport(() => ({ status: 500 }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useToggleToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ serverId: SERVER_ID, isEnabled: true });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Update failed", "Could not update the tool server.");
  });

  it("removes a server and names it in the notice", async () => {
    const { transport, calls } = stubTransport(() => ({ status: 204 }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useRemoveToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ serverId: SERVER_ID, name: "probe" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({
      method: "DELETE",
      url: `https://api.test/api/users/me/ai-assistant/mcp-servers/${SERVER_ID}/`,
    });
    expect(notifyInfo).toHaveBeenCalledWith(SERVER_REMOVED_TITLE, "probe removed.");
  });

  it("reports a failed removal, with a fallback", async () => {
    const { transport } = stubTransport(() => ({ status: 500 }));
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useRemoveToolServer(), { wrapper: wrapper() });
    act(() => {
      result.current.mutate({ serverId: SERVER_ID, name: "probe" });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith(SERVER_REMOVE_FAILED_TITLE, "Could not remove the tool server.");
  });
});
