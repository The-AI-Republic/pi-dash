// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ToastHost } from "@pidash/kit";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { getToastManager } from "../shell/toaster.js";
import { TimezonePicker } from "./TimezonePicker.js";
import { useUpdateTimezone } from "./timezone.js";

const USER_ID = "123e4567-e89b-12d3-a456-426614174000";

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

describe("useUpdateTimezone", () => {
  afterEach(() => {
    resetAppClient();
  });

  function MutationProbe({ onDone }: { onDone: (timezone: string) => void }): React.ReactElement {
    const update = useUpdateTimezone({ onSuccess: onDone });
    return (
      <button type="button" onClick={() => update.mutate("Asia/Tokyo")}>
        save
      </button>
    );
  }

  it("persists the zone, confirms and notifies the caller", async () => {
    const user = userEvent.setup();
    const { queryClient, calls } = harness((url, init) => {
      if (init.method === "PATCH") {
        const body = JSON.parse(init.body ?? "{}") as { user_timezone?: string };
        return {
          status: 200,
          body: JSON.stringify({
            id: USER_ID,
            email: "contract.tester@example.com",
            user_timezone: body.user_timezone ?? "",
          }),
        };
      }
      return { status: 200, body: JSON.stringify(ZONES) };
    });
    const seen: string[] = [];
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <MutationProbe onDone={(timezone) => seen.push(timezone)} />
      </QueryClientProvider>
    );
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "save" }));
    });
    await waitFor(() => expect(seen).toEqual(["Asia/Tokyo"]));
    expect(calls.some((call) => call.url.endsWith("/api/users/me/") && call.method === "PATCH")).toBe(true);
    expect(JSON.parse(calls.find((call) => call.method === "PATCH")?.body ?? "{}")).toEqual({
      user_timezone: "Asia/Tokyo",
    });
    expect(await screen.findByText("Timezone updated successfully.")).toBeInTheDocument();
  });

  it("toasts failures and reports them to the caller", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(() => ({ status: 500, body: '{"detail":"boom"}' }));
    const errors: Error[] = [];
    function FailingProbe(): React.ReactElement {
      const update = useUpdateTimezone({ onError: (error) => errors.push(error) });
      return (
        <button type="button" onClick={() => update.mutate("Asia/Tokyo")}>
          save
        </button>
      );
    }
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <FailingProbe />
      </QueryClientProvider>
    );
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "save" }));
    });
    await waitFor(() => expect(errors).toHaveLength(1));
    expect(await screen.findByText("Couldn't update the timezone")).toBeInTheDocument();
  });
});
