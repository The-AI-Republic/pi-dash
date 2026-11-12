// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import type { MiddlewareContext, TransportResponse } from "../client.js";
import {
  createCloudPaywallMiddleware,
  PAYWALL_PRICING_PATH,
  paywallMessage,
  type PaywallPrompt,
} from "./cloud-paywall.js";

type Next = (request: MiddlewareContext) => Promise<TransportResponse>;

const QUOTA_BODY = JSON.stringify({
  error: "quota_exceeded",
  code: "free_issue_creation_limit_exceeded",
  limit: 500,
  used: 500,
  upgrade_url: "https://airepublic.test/upgrade",
});

function answer(status: number, body: string, textImpl?: () => Promise<string>): TransportResponse {
  return {
    status,
    url: "https://api.test/api/issues/",
    headers: { get: () => null },
    text: textImpl ?? (async () => body),
  };
}

const call = (url = "https://api.test/api/issues/"): MiddlewareContext => ({ method: "POST", url, headers: {} });

type MockPaywallPrompt = PaywallPrompt & {
  confirm: ReturnType<typeof vi.fn>;
  openUpgrade: ReturnType<typeof vi.fn>;
};

function promptOf(overrides: Partial<MockPaywallPrompt> = {}): MockPaywallPrompt {
  return {
    confirm: vi.fn(async () => true),
    openUpgrade: vi.fn(async () => undefined),
    ...overrides,
  };
}

describe("paywallMessage", () => {
  it("names the limit and its numbers", () => {
    const message = paywallMessage(500, 500);
    expect(message.toLowerCase()).toContain("limit");
    expect(message).toContain("500");
  });
});

describe("createCloudPaywallMiddleware", () => {
  it("passes non-402 answers through without reading the body", async () => {
    const text = vi.fn(async () => QUOTA_BODY);
    const next = vi.fn(async () => answer(201, "", text));
    const prompt = promptOf();
    const response = await createCloudPaywallMiddleware(prompt)(call(), next as Next);
    expect(response.status).toBe(201);
    expect(text).not.toHaveBeenCalled();
    expect(prompt.confirm).not.toHaveBeenCalled();
  });

  it("prompts once on the exact quota shape and opens the payload address", async () => {
    const next = vi.fn(async () => answer(402, QUOTA_BODY));
    const prompt = promptOf();
    const response = await createCloudPaywallMiddleware(prompt)(call(), next as Next);
    expect(prompt.confirm).toHaveBeenCalledTimes(1);
    expect(String(prompt.confirm.mock.calls[0]?.[0]).toLowerCase()).toContain("limit");
    expect(prompt.openUpgrade).toHaveBeenCalledTimes(1);
    expect(prompt.openUpgrade).toHaveBeenCalledWith("https://airepublic.test/upgrade");
    expect(response.status).toBe(402);
    expect(await response.text()).toBe(QUOTA_BODY);
  });

  it("falls back to the pricing page when the payload carries no address", async () => {
    const body = JSON.stringify({
      error: "quota_exceeded",
      code: "free_issue_creation_limit_exceeded",
      limit: 500,
      used: 500,
    });
    const next = vi.fn(async () => answer(402, body));
    const prompt = promptOf();
    await createCloudPaywallMiddleware(prompt)(call(), next as Next);
    expect(prompt.openUpgrade).toHaveBeenCalledWith("https://api.test/pricing");
    expect(PAYWALL_PRICING_PATH).toBe("/pricing");
  });

  it("opens nothing when the user dismisses", async () => {
    const next = vi.fn(async () => answer(402, QUOTA_BODY));
    const prompt = promptOf({ confirm: vi.fn(async () => false) });
    const response = await createCloudPaywallMiddleware(prompt)(call(), next as Next);
    expect(prompt.confirm).toHaveBeenCalledTimes(1);
    expect(prompt.openUpgrade).not.toHaveBeenCalled();
    expect(response.status).toBe(402);
  });

  it("never prompts without the exact shape", async () => {
    for (const body of [
      "",
      "not json",
      JSON.stringify({ detail: "nope" }),
      JSON.stringify({ error: "quota_exceeded" }),
    ]) {
      const next = vi.fn(async () => answer(402, body));
      const prompt = promptOf();
      const response = await createCloudPaywallMiddleware(prompt)(call(), next as Next);
      expect(prompt.confirm).not.toHaveBeenCalled();
      expect(response.status).toBe(402);
      if (body === "not json") {
        expect(response).toBe(await next.mock.results[0]?.value);
      } else {
        expect(await response.text()).toBe(body);
      }
    }
  });

  it("leaves the answer untouched when its body cannot be read", async () => {
    const unreadable = answer(402, "", async () => {
      throw new Error("torn body");
    });
    const next = vi.fn(async () => unreadable);
    const prompt = promptOf();
    const response = await createCloudPaywallMiddleware(prompt)(call(), next as Next);
    expect(response).toBe(unreadable);
    expect(prompt.confirm).not.toHaveBeenCalled();
  });

  it("guards a second prompt while one is still showing", async () => {
    let release!: (value: boolean) => void;
    const gate = new Promise<boolean>((resolve) => {
      release = resolve;
    });
    const next = vi.fn(async () => answer(402, QUOTA_BODY));
    const prompt = promptOf({ confirm: vi.fn(() => gate) });
    const middleware = createCloudPaywallMiddleware(prompt);
    const first = middleware(call(), next as Next);
    await new Promise((resolve) => setTimeout(resolve, 5));
    const second = await middleware(call(), next as Next);
    expect(prompt.confirm).toHaveBeenCalledTimes(1);
    expect(second.status).toBe(402);
    release(true);
    expect((await first).status).toBe(402);
    expect(prompt.openUpgrade).toHaveBeenCalledTimes(1);
  });

  it("still answers the refusal when the tab open is blocked", async () => {
    const next = vi.fn(async () => answer(402, QUOTA_BODY));
    const prompt = promptOf({
      openUpgrade: vi.fn(async () => {
        throw new Error("popup blocked");
      }),
    });
    const response = await createCloudPaywallMiddleware(prompt)(call(), next as Next);
    expect(response.status).toBe(402);
    expect(await response.text()).toBe(QUOTA_BODY);
  });

  it("prefers the payload address even when the request origin is unusable", async () => {
    const next = vi.fn(async () => answer(402, QUOTA_BODY));
    const prompt = promptOf();
    await createCloudPaywallMiddleware(prompt)(call("::::"), next as Next);
    expect(prompt.openUpgrade).toHaveBeenCalledWith("https://airepublic.test/upgrade");
  });

  it("hands the bare pricing path to the opener when no origin can be derived", async () => {
    const body = JSON.stringify({
      error: "quota_exceeded",
      code: "free_issue_creation_limit_exceeded",
      limit: 500,
      used: 500,
    });
    const next = vi.fn(async () => answer(402, body));
    const prompt = promptOf();
    await createCloudPaywallMiddleware(prompt)(call("::::"), next as Next);
    expect(prompt.openUpgrade).toHaveBeenCalledWith("/pricing");
  });
});
