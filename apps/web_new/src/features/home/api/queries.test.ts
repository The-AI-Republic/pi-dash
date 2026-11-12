// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home's assistant-card reads delegate to the assistant feature's query
// options, so the card and the assistant surfaces share one cache entry
// per workspace (NEWFRONT-603).
import { describe, expect, it, vi } from "vitest";

import { homeThreadsQueryOptions } from "./queries.js";

function stubClient(body: unknown) {
  const getJson = vi.fn(async () => body);
  const client = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
  return { client, getJson };
}

describe("homeThreadsQueryOptions", () => {
  it("keys threads per workspace under the assistant subtree with list freshness", () => {
    const options = homeThreadsQueryOptions(stubClient([]).client, "acme", true);
    expect(options.queryKey).toEqual(["ws", "acme", "assistant", "threads"]);
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
