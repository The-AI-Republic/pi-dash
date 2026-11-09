// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Usage-quota contract against a stub client (no backend, NEWPIDASH-149):
// the path, the verbatim Hub shape, and the optional Apps allowance.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { getUsageQuota } from "./openhub.js";

const QUOTA_BODY = {
  plan: "free",
  llm: {
    freeTokensUsed: 123_456,
    freeTokensLimit: 2_000_000,
    freeModels: ["deepseek/deepseek-v4-flash"],
  },
  apps: { freeCallsUsed: 5, freeCallsLimit: 150 },
  resetsAt: "2026-11-10T00:00:00+00:00",
};

function stubClient(getJson: ApiClient["getJson"]): ApiClient {
  return {
    getJson,
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
  } as ApiClient;
}

describe("getUsageQuota", () => {
  it("reads the quota on its own path and parses the Hub shape", async () => {
    const calls: string[] = [];
    const client = stubClient((async (path: string) => {
      calls.push(path);
      return QUOTA_BODY;
    }) as ApiClient["getJson"]);
    const quota = await getUsageQuota(client);
    expect(calls).toEqual(["/api/openhub/usage/quota/"]);
    expect(quota.plan).toBe("free");
    expect(quota.llm.freeTokensUsed).toBe(123_456);
    expect(quota.llm.freeTokensLimit).toBe(2_000_000);
    expect(quota.llm.freeModels).toEqual(["deepseek/deepseek-v4-flash"]);
    expect(quota.apps).toEqual({ freeCallsUsed: 5, freeCallsLimit: 150 });
    expect(quota.resetsAt).toBe("2026-11-10T00:00:00+00:00");
  });

  it("accepts a Hub answer with no Apps allowance", async () => {
    const client = stubClient((async () => {
      const { apps: _apps, ...withoutApps } = QUOTA_BODY;
      return withoutApps;
    }) as ApiClient["getJson"]);
    const quota = await getUsageQuota(client);
    expect(quota.apps).toBeUndefined();
    expect(quota.plan).toBe("free");
  });

  it("rejects a Hub answer missing the LLM allowance", async () => {
    const client = stubClient((async () => ({ plan: "free", resetsAt: QUOTA_BODY.resetsAt })) as ApiClient["getJson"]);
    await expect(getUsageQuota(client)).rejects.toThrow();
  });
});
