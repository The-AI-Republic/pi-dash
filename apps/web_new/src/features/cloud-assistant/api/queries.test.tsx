// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reads behind the cloud tab (CLOUD-032/037): the paths each query calls,
// the key it fills, and its freshness.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { cloudAssistantKeys } from "./keys.js";
import { cloudAssistantConfigQueryOptions, cloudConnectionsQueryOptions, useCloudAssistantConfig } from "./queries.js";

function fakeClient() {
  const getJson = vi.fn(async (_path: string): Promise<unknown> => ({}));
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("cloud assistant keys", () => {
  it("are user-level, not workspace-scoped", () => {
    expect(cloudAssistantKeys.connections()).toEqual(["cloud-assistant", "connections"]);
    expect(cloudAssistantKeys.config()).toEqual(["cloud-assistant", "config"]);
  });
});

describe("cloudConnectionsQueryOptions", () => {
  it("reads the connection list", async () => {
    const { client, getJson } = fakeClient();
    const options = cloudConnectionsQueryOptions(client);
    expect(options.queryKey).toEqual(cloudAssistantKeys.connections());
    expect(options.staleTime).toBe(REFERENCE_STALE_TIME_MS);
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/ai-assistant/connections/");
  });
});

describe("cloudAssistantConfigQueryOptions", () => {
  it("reads the provider config", async () => {
    const { client, getJson } = fakeClient();
    const options = cloudAssistantConfigQueryOptions(client);
    expect(options.queryKey).toEqual(cloudAssistantKeys.config());
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/users/me/ai-assistant/config/");
  });
});

describe("useCloudAssistantConfig", () => {
  it("reads the lane config for the assistant entry points", async () => {
    createAppClient("https://api.test", (async () => ({
      status: 200,
      url: "https://api.test/api/users/me/ai-assistant/config/",
      headers: { get: () => "application/json" },
      text: async () =>
        JSON.stringify({
          provider_kind: "openai_compatible",
          base_url: "",
          model_name: "",
          has_api_key: false,
          last_verified_at: null,
          credential_mode: "openhub",
          has_openhub_access: true,
        }),
    })) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children?: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
    );
    const { result } = renderHook(() => useCloudAssistantConfig(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toMatchObject({ credential_mode: "openhub", has_openhub_access: true });
    resetAppClient();
  });
});
