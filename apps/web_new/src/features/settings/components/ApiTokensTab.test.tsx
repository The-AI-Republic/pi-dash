// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Token tab body (parity/specs/profile.md §9, PROF-020; §41 seam): the
// seam mounts the workspace-settings token page on demand, so this module
// carries only a quiet fallback and never rebuilds the token screens.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FULL_RUN_TEST_TIMEOUT_MS } from "../../../testing/testBudget.js";
import { ApiTokensTab } from "./ApiTokensTab.js";

// The tab suspends on a lazy chunk, so the first paint pays for the module
// import. Wait with the suite's measured load allowance instead of
// testing-library's fixed one-second default, which the global testTimeout
// does not relax.
const CHUNK_WAIT = { timeout: FULL_RUN_TEST_TIMEOUT_MS };

beforeEach(() => {
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => (url.endsWith("/auth/get-csrf-token/") ? '{"csrf_token":"csrf"}' : "[]"),
  })) as never);
});

afterEach(() => resetAppClient());

function renderTab() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiTokensTab />
    </QueryClientProvider>
  );
}

describe("ApiTokensTab", () => {
  it("shows a busy fallback until the token page chunk arrives", () => {
    renderTab();
    const fallback = screen.getByTestId("api-tokens-tab-fallback");
    expect(fallback).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByRole("heading", { name: "Personal Access Tokens" })).not.toBeInTheDocument();
  });

  it("mounts the workspace-built token page once loaded", async () => {
    renderTab();
    expect(await screen.findByRole("heading", { name: "Personal Access Tokens" }, CHUNK_WAIT)).toBeInTheDocument();
    expect(screen.queryByTestId("api-tokens-tab-fallback")).not.toBeInTheDocument();
  });
});
