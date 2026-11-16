// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { TimezonePicker } from "./index.js";

const ZONES = {
  timezones: [
    { utc_offset: "+09:00", gmt_offset: "GMT+9", value: "Asia/Tokyo", label: "Tokyo" },
    { utc_offset: "+11:00", gmt_offset: "GMT+11", value: "Australia/Sydney", label: "Sydney" },
  ],
};

function jsonResponse(url: string, body: string, status = 200) {
  return { status, url, headers: { get: () => "application/json" }, text: async () => body };
}

function harness(
  routes: (url: string, init: { method: string; body?: string | undefined }) => { status: number; body: string }
) {
  const calls: Array<{ url: string; method: string; body?: string | undefined }> = [];
  const transport = async (url: string, init: { method: string; body?: string }) => {
    calls.push({ url, method: init.method, body: init.body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, '{"csrf_token":"csrf"}');
    const route = routes(url, init);
    return jsonResponse(url, route.body, route.status);
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return { queryClient, calls };
}

describe("TimezonePicker", () => {
  afterEach(() => {
    resetAppClient();
  });

  it("searches zones, marks the selection and reports picks", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(() => ({ status: 200, body: JSON.stringify(ZONES) }));
    const onSelect = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <TimezonePicker value="Australia/Sydney" onSelect={onSelect} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: /Tokyo/ })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Sydney/ })).toHaveAttribute("aria-selected", "true");
    await user.type(screen.getByLabelText("Search timezones"), "tok");
    expect(screen.queryByRole("option", { name: /Sydney/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /Tokyo/ }));
    expect(onSelect).toHaveBeenCalledWith("Asia/Tokyo");
  });

  it("names empty searches and failed loads", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(() => ({ status: 200, body: JSON.stringify(ZONES) }));
    render(
      <QueryClientProvider client={queryClient}>
        <TimezonePicker value="UTC" onSelect={() => undefined} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: /Tokyo/ })).toBeInTheDocument();
    await user.type(screen.getByLabelText("Search timezones"), "nowhere");
    expect(screen.getByText(/No timezones match/)).toBeInTheDocument();
  });

  it("reports a failed zone load", async () => {
    const { queryClient } = harness(() => ({ status: 500, body: '{"detail":"boom"}' }));
    render(
      <QueryClientProvider client={queryClient}>
        <TimezonePicker value="UTC" onSelect={() => undefined} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load the timezone list.");
  });
});
