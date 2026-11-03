// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Week-start write (parity/specs/profile.md §7, PROF-017): PATCH the
// profile with the zero-based index and fold the server answer into the
// session cache the calendars read.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/index.js";

import { useSaveWeekStart } from "./api.js";

const PROFILE = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  user: "223e4567-e89b-12d3-a456-426614174001",
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: true,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 1,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function harness() {
  const patches: unknown[] = [];
  const transport = async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    if (init.method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as { start_of_the_week?: number };
      patches.push(body);
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ ...PROFILE, start_of_the_week: body.start_of_the_week ?? 0 }),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(PROFILE),
    };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  }
  return { patches, queryClient, wrapper };
}

afterEach(() => resetAppClient());

describe("useSaveWeekStart", () => {
  it("writes the index and seeds the profile cache with the answer", async () => {
    const { patches, queryClient, wrapper } = harness();
    const { result } = renderHook(() => useSaveWeekStart(), { wrapper });
    result.current.mutate(6);
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(patches).toEqual([{ start_of_the_week: 6 }]);
    expect(queryClient.getQueryData(sessionKeys.profile)).toMatchObject({ start_of_the_week: 6 });
  });

  it("surfaces a failed write to the caller", async () => {
    const transport = async (url: string, init: { method: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => '{"csrf_token":"csrf"}',
        };
      }
      return init.method === "PATCH"
        ? { status: 500, url, headers: { get: () => "application/json" }, text: async () => '{"detail":"boom"}' }
        : { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(PROFILE) };
    };
    createAppClient("https://api.test", transport as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    function wrapper({ children }: { children?: unknown }): React.ReactElement {
      return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
    }
    const { result } = renderHook(() => useSaveWeekStart(), { wrapper });
    result.current.mutate(2);
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error?.message).toBeTruthy();
  });
});
