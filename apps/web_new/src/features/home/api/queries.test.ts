// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { homeThreadsQueryOptions, homeAssistantConfigQueryOptions } from "./queries.js";

function stubClient(body: unknown) {
  const getJson = vi.fn(async () => body);
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("homeThreadsQueryOptions", () => {
  it("keys threads per workspace with list freshness", () => {
    const options = homeThreadsQueryOptions(stubClient([]).client, "acme", true);
    expect(options.queryKey).toEqual(["ws", "acme", "home", "assistant-threads"]);
    expect(options.staleTime).toBe(30_000);
    expect(options.enabled).toBe(true);
  });

  it("passes the enabled flag through", () => {
    expect(homeThreadsQueryOptions(stubClient([]).client, "acme", false).enabled).toBe(false);
  });

  it("lists threads through the contract", async () => {
    const { client, getJson } = stubClient([{ id: "thread-1" }]);
    const options = homeThreadsQueryOptions(client, "acme", true);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([{ id: "thread-1" }]);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/ai-assistant/threads/");
  });
});

describe("homeAssistantConfigQueryOptions", () => {
  it("keys the config per workspace with reference freshness", () => {
    const options = homeAssistantConfigQueryOptions(stubClient({}).client, "acme", true);
    expect(options.queryKey).toEqual(["ws", "acme", "home", "assistant-config"]);
    expect(options.staleTime).toBe(5 * 60_000);
    expect(options.enabled).toBe(true);
  });

  it("passes the enabled flag through", () => {
    expect(homeAssistantConfigQueryOptions(stubClient({}).client, "acme", false).enabled).toBe(false);
  });

  it("reads the model config through the contract", async () => {
    const { client, getJson } = stubClient({ model_name: "pilot" });
    const options = homeAssistantConfigQueryOptions(client, "acme", true);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ model_name: "pilot" });
    expect(getJson).toHaveBeenCalledWith("/api/users/me/ai-assistant/config/");
  });
});
