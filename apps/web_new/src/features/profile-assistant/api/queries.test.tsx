// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI-assistant reads (parity/specs/profile.md §10): each hook calls its own
// own-record endpoint, caches under the assistant subtree, and never
// refetches on focus (PROF-027).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport, TransportInit } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { assistantSettingsKeys } from "./keys.js";
import { useAssistantLlm, useAssistantServers, useAssistantVoice } from "./queries.js";

const LLM = {
  provider_kind: "anthropic",
  base_url: "",
  model_name: "claude",
  has_api_key: true,
  last_verified_at: null,
};

const VOICE = {
  base_url: "https://stt.test/v1",
  model_name: "whisper-1",
  has_api_key: false,
  last_verified_at: null,
  enabled: true,
};

const SERVER_ID = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";
const SERVERS = [
  {
    id: SERVER_ID,
    name: "probe",
    url: "https://mcp.test",
    has_auth_header: true,
    tool_prefix: "probe",
    effective_tool_prefix: "probe",
    is_enabled: true,
    created_at: "2026-10-19T00:00:00Z",
    updated_at: "2026-10-19T00:00:00Z",
  },
];

function stubTransport() {
  const urls: string[] = [];
  const transport: Transport = async (url: string, _init: TransportInit) => {
    urls.push(url);
    const body = url.endsWith("/ai-assistant/config/") ? LLM : url.endsWith("/stt-config/") ? VOICE : SERVERS;
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    };
  };
  return { transport, urls };
}

function harness() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

afterEach(() => resetAppClient());

describe("assistant reads", () => {
  it("reads the member's model config under the assistant key", async () => {
    const { transport, urls } = stubTransport();
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useAssistantLlm(), { wrapper: harness() });
    await waitFor(() => expect(result.current.data).toEqual(LLM));
    expect(urls).toEqual(["https://api.test/api/users/me/ai-assistant/config/"]);
    expect(assistantSettingsKeys.llm).toEqual(["profile-ai-assistant", "llm"]);
  });

  it("reads the dictation config, including its instance switch", async () => {
    const { transport, urls } = stubTransport();
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useAssistantVoice(), { wrapper: harness() });
    await waitFor(() => expect(result.current.data).toEqual(VOICE));
    expect(urls).toEqual(["https://api.test/api/users/me/ai-assistant/stt-config/"]);
    expect(result.current.data?.enabled).toBe(true);
  });

  it("reads the tool-server registry as a list", async () => {
    const { transport, urls } = stubTransport();
    createAppClient("https://api.test", transport);
    const { result } = renderHook(() => useAssistantServers(), { wrapper: harness() });
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    expect(urls).toEqual(["https://api.test/api/users/me/ai-assistant/mcp-servers/"]);
    expect(result.current.data?.[0]?.name).toBe("probe");
  });
});
