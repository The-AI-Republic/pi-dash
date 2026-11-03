// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { useUpdateProfile } from "./profile.js";

function wrapper(client: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Provider({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  };
}

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

afterEach(() => {
  resetAppClient();
});

const PROFILE_FIXTURE = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  user: "223e4567-e89b-12d3-a456-426614174001",
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: false,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "full",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
};

describe("useUpdateProfile", () => {
  it("patches the profile and refreshes the cached profile", async () => {
    const seen: string[] = [];
    createAppClient(
      "https://api.test",
      stubTransport(async (url, init) => {
        seen.push(`${init.method} ${url}`);
        if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
        return { status: 200, body: JSON.stringify({ ...PROFILE_FIXTURE, role: "developer" }), url };
      })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["session", "profile"], { ...PROFILE_FIXTURE });
    const { result } = renderHook(() => useUpdateProfile(), { wrapper: wrapper(queryClient) });

    let updated: unknown;
    await act(async () => {
      updated = await result.current.mutateAsync({ role: "developer" });
    });
    expect((updated as { role: string }).role).toBe("developer");
    expect(seen).toContain("PATCH https://api.test/api/users/me/profile/");
    expect(queryClient.getQueryState(["session", "profile"])?.isInvalidated).toBe(true);
  });
});
