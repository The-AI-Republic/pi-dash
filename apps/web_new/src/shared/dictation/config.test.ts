// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { SttConfig } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ getSttConfig: vi.fn() }));
vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

const { createAppClient, resetAppClient } = await import("../../core/api/client.js");
const { dictationOffered, sttConfigKeys, sttConfigQueryOptions, useDictationConfig } = await import("./config.js");

function config(overrides: Partial<SttConfig> = {}): SttConfig {
  return {
    base_url: "https://stt.test",
    model_name: "whisper",
    has_api_key: true,
    last_verified_at: null,
    enabled: true,
    ...overrides,
  };
}

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: QueryClient;
  children?: React.ReactNode;
}>;

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
  contracts.getSttConfig.mockReset();
});

afterEach(() => resetAppClient());

describe("dictationOffered", () => {
  it("needs both the operator switch and a saved key", () => {
    expect(dictationOffered(config())).toBe(true);
    expect(dictationOffered(config({ enabled: false }))).toBe(false);
    expect(dictationOffered(config({ enabled: undefined }))).toBe(false);
    expect(dictationOffered(config({ has_api_key: false }))).toBe(false);
    expect(dictationOffered(undefined)).toBe(false);
  });
});

describe("sttConfigQueryOptions", () => {
  it("keys the read under the assistant namespace and reuses the client", async () => {
    const client = { baseUrl: "https://api.test" } as never;
    const options = sttConfigQueryOptions(client);
    expect(options.queryKey).toEqual(sttConfigKeys.all());
    expect(options.staleTime).toBe(60_000);
    contracts.getSttConfig.mockResolvedValue(config());
    await options.queryFn!({} as never);
    expect(contracts.getSttConfig).toHaveBeenCalledWith(client);
  });
});

describe("useDictationConfig", () => {
  it("stays hidden while loading and shows the read once it lands", async () => {
    contracts.getSttConfig.mockResolvedValue(config());
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
      React.createElement(Provider, { client }, children);
    const { result } = renderHook(() => useDictationConfig(), { wrapper });
    expect(dictationOffered(result.current.data)).toBe(false);
    await waitFor(() => expect(dictationOffered(result.current.data)).toBe(true));
  });

  it("stays hidden after a failed read", async () => {
    contracts.getSttConfig.mockRejectedValue(new Error("offline"));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
      React.createElement(Provider, { client }, children);
    const { result } = renderHook(() => useDictationConfig(), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(dictationOffered(result.current.data)).toBe(false);
  });

  it("does not read when disabled", () => {
    const client = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
      React.createElement(Provider, { client }, children);
    renderHook(() => useDictationConfig(false), { wrapper });
    expect(contracts.getSttConfig).not.toHaveBeenCalled();
  });
});
