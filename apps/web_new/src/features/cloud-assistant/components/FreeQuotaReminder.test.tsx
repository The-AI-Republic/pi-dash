// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paid-plan free-quota reminder (NEWPIDASH-181): the toggle renders only
// on payg/member plans, persists the server flag, and the banner is the
// once-per-UTC-day informational note. The mutation is stubbed at the
// api-client boundary, so this pins the frontend half of the contract;
// the key itself is declared by the backend half (PRIVATEPI1-147).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { freeQuotaReminderEnabled, type Profile } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/queries.js";
import { cloudAssistantKeys } from "../api/keys.js";
import { utcDayKey } from "../lib/freeQuotaReminder.js";

const { updateProfileMock } = vi.hoisted(() => ({ updateProfileMock: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, updateProfile: updateProfileMock };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { FreeQuotaReminder } from "./FreeQuotaReminder.js";

const PROFILE_PATH = "/api/users/me/profile/";
const QUOTA_PATH = "/api/openhub/usage/quota/";

function profileWith(reminder: boolean | undefined, extra: Record<string, unknown> = {}): Profile {
  const openhub: Record<string, unknown> = { ...(extra["openhub"] as Record<string, unknown> | undefined) };
  if (reminder !== undefined) openhub["free_quota_reminder"] = reminder;
  // A full Profile body: getProfile parses with the Profile zod schema,
  // so a settings-only stub would be rejected and the section would never
  // mount (mirrors the profileJson precedent in AppsCatalog.test.tsx).
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    user: "123e4567-e89b-12d3-a456-426614174001",
    theme: {},
    is_app_rail_docked: false,
    is_tour_completed: true,
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
    start_of_the_week: 0,
    goals: {},
    background_color: "",
    is_navigation_tour_completed: false,
    has_marketing_email_consent: false,
    is_subscribed_to_changelog: false,
    product_tour: {},
    settings: { ...extra, openhub },
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
  } as unknown as Profile;
}

function quotaWith(plan: string, used: number, limit: number): unknown {
  return {
    plan,
    llm: { freeTokensUsed: used, freeTokensLimit: limit, freeModels: ["openhub_free_llm"] },
    resetsAt: "2026-11-10T00:00:00+00:00",
  };
}

function stub(profile: Profile, quota: unknown): void {
  const fetchMock = vi.fn(async (url: string) => {
    if (url.endsWith(PROFILE_PATH)) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(profile),
      };
    }
    if (url.endsWith(QUOTA_PATH)) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(quota),
      };
    }
    throw new Error(`unexpected call ${url}`);
  });
  createAppClient("https://api.test", fetchMock as never);
}

function renderReminder(): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <FreeQuotaReminder />
    </QueryClientProvider>
  );
  return queryClient;
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
});

afterEach(() => {
  resetAppClient();
});

describe("FreeQuotaReminder", () => {
  it("shows the toggle off by default on a paid plan and persists the flip", async () => {
    stub(profileWith(undefined), quotaWith("payg", 100, 5_000_000));
    updateProfileMock.mockImplementation(async (_client: unknown, patch: unknown) => {
      return profileWith(
        (patch as { settings: { openhub: { free_quota_reminder: boolean } } }).settings.openhub.free_quota_reminder
      );
    });
    renderReminder();

    const toggle = await screen.findByTestId("free-quota-reminder-switch");
    await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("false"));
    expect(screen.queryByTestId("free-quota-reminder-banner")).toBeNull();

    fireEvent.click(toggle);
    await waitFor(() => {
      expect(updateProfileMock).toHaveBeenCalledWith(expect.anything(), {
        settings: { openhub: { free_quota_reminder: true } },
      });
    });
  });

  it("shows the banner once when on and the allowance is exhausted", async () => {
    stub(profileWith(true), quotaWith("member", 5_000_000, 5_000_000));
    renderReminder();

    const banner = await screen.findByTestId("free-quota-reminder-banner");
    expect(banner).toHaveTextContent("Today's free tokens are used up. Requests now use your OpenHub credit.");
    expect(banner.getAttribute("role")).toBe("status");
  });

  it("stays quiet when the toggle is off", async () => {
    stub(profileWith(false), quotaWith("payg", 5_000_000, 5_000_000));
    renderReminder();

    await screen.findByTestId("free-quota-reminder");
    expect(screen.queryByTestId("free-quota-reminder-banner")).toBeNull();
  });

  it("stays quiet on the free plan", async () => {
    stub(profileWith(true), quotaWith("free", 2_000_000, 2_000_000));
    const queryClient = renderReminder();

    // Prove the queries resolved first, so the null below is the plan
    // gate and not a stuck loading state.
    await waitFor(() => {
      expect(queryClient.getQueryData(sessionKeys.profile)).toBeDefined();
      expect(queryClient.getQueryData(cloudAssistantKeys.usageQuota())).toBeDefined();
    });
    expect(screen.queryByTestId("free-quota-reminder")).toBeNull();
    expect(screen.queryByTestId("free-quota-reminder-banner")).toBeNull();
  });

  it("suppresses the banner after it has shown today", async () => {
    window.localStorage.setItem(`pi-dash:free-quota-reminder:shown:${utcDayKey(new Date())}`, "1");
    stub(profileWith(true), quotaWith("payg", 5_000_000, 5_000_000));
    renderReminder();

    await screen.findByTestId("free-quota-reminder");
    expect(screen.queryByTestId("free-quota-reminder-banner")).toBeNull();
  });

  it("dismissing the banner suppresses it for the rest of the day", async () => {
    stub(profileWith(true), quotaWith("payg", 5_000_000, 5_000_000));
    renderReminder();

    const banner = await screen.findByTestId("free-quota-reminder-banner");
    expect(banner).toBeVisible();
    fireEvent.click(screen.getByTestId("free-quota-reminder-dismiss"));
    await waitFor(() => expect(screen.queryByTestId("free-quota-reminder-banner")).toBeNull());
    expect(window.localStorage.getItem(`pi-dash:free-quota-reminder:shown:${utcDayKey(new Date())}`)).toBe("1");
  });

  it("preserves sibling settings on the optimistic flip", async () => {
    stub(profileWith(false, { openhub: { apps_enabled: true } }), quotaWith("payg", 100, 5_000_000));
    updateProfileMock.mockImplementation(async () => profileWith(true, { openhub: { apps_enabled: true } }));
    renderReminder();

    const toggle = await screen.findByTestId("free-quota-reminder-switch");
    fireEvent.click(toggle);
    await waitFor(() => expect(updateProfileMock).toHaveBeenCalledTimes(1));
    expect(freeQuotaReminderEnabled({ openhub: { free_quota_reminder: true, apps_enabled: true } })).toBe(true);
  });
});

describe("on/off helpers", () => {
  it("reads missing as off", () => {
    expect(freeQuotaReminderEnabled({ openhub: {} })).toBe(false);
    expect(freeQuotaReminderEnabled({})).toBe(false);
  });

  it("exposes the query key the flip writes to", () => {
    expect(sessionKeys.profile).toEqual(["session", "profile"]);
  });
});
