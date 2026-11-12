// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { useWeekStart } from "./weekStart.js";

function Probe(): React.ReactElement {
  return <span data-testid="week-start">{useWeekStart()}</span>;
}

function renderProbe(queryClient: QueryClient): void {
  render(
    <QueryClientProvider client={queryClient}>
      <Probe />
    </QueryClientProvider>
  );
}

describe("useWeekStart", () => {
  afterEach(() => {
    resetAppClient();
    vi.unstubAllGlobals();
  });

  it("reads the profile's first day of week", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("must not fetch: the profile is cached");
    }) as never);
    const queryClient = new QueryClient();
    queryClient.setQueryData(sessionKeys.profile, { start_of_the_week: 1 });
    renderProbe(queryClient);
    expect(screen.getByTestId("week-start")).toHaveTextContent("1");
  });

  it("normalizes an out-of-range profile value to Sunday", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("must not fetch: the profile is cached");
    }) as never);
    const queryClient = new QueryClient();
    queryClient.setQueryData(sessionKeys.profile, { start_of_the_week: 9 });
    renderProbe(queryClient);
    expect(screen.getByTestId("week-start")).toHaveTextContent("0");
  });

  it("falls back to Sunday while the profile fails to load", () => {
    const silence = vi.spyOn(console, "error").mockImplementation(() => {});
    createAppClient("https://api.test", (async () => {
      throw new Error("offline");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    renderProbe(queryClient);
    expect(screen.getByTestId("week-start")).toHaveTextContent("0");
    silence.mockRestore();
  });
});
