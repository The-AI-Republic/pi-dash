// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Free-lane usage meter (NEWPIDASH-149): the sentence when the quota
// reads, and nothing while loading or failing — a broken quota must
// never block the page.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FreeTokenMeter } from "./FreeTokenMeter.js";

const QUOTA = "/api/openhub/usage/quota/";

const QUOTA_BODY = {
  plan: "free",
  llm: {
    freeTokensUsed: 123_456,
    freeTokensLimit: 2_000_000,
    freeModels: ["deepseek/deepseek-v4.1-flash-1017"],
  },
  apps: { freeCallsUsed: 5, freeCallsLimit: 150 },
  resetsAt: "2026-11-10T00:00:00+00:00",
};

function stubQuota(body: unknown, status = 200): void {
  createAppClient("https://api.test", (async (url: string) => {
    if (!url.endsWith(QUOTA)) throw new Error(`unexpected call ${url}`);
    return {
      status,
      url,
      headers: { get: () => (body === undefined ? null : "application/json") },
      text: async () => JSON.stringify(body),
    };
  }) as never);
}

function renderMeter(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <FreeTokenMeter />
    </QueryClientProvider>
  );
}

function localHHMM(iso: string): string {
  const instant = new Date(iso);
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${pad(instant.getHours())}:${pad(instant.getMinutes())}`;
}

afterEach(() => {
  resetAppClient();
});

describe("FreeTokenMeter", () => {
  it("states today's usage with the local reset", async () => {
    stubQuota(QUOTA_BODY);
    renderMeter();
    expect(await screen.findByTestId("free-token-meter")).toHaveTextContent(
      `123.5K of 2M free tokens used today, resets at ${localHHMM(QUOTA_BODY.resetsAt)} (local)`
    );
  });

  it("renders nothing while the quota loads", () => {
    stubQuota(QUOTA_BODY);
    renderMeter();
    expect(screen.queryByTestId("free-token-meter")).toBeNull();
  });

  it("hides when the quota read fails", async () => {
    stubQuota({ error: "openhub_unavailable", detail: "OpenHub is unavailable right now." }, 503);
    renderMeter();
    await waitFor(() => expect(screen.queryByTestId("free-token-meter")).toBeNull());
    expect(document.body.textContent).not.toContain("free tokens used");
  });
});
