// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../api/client.js";
import { LanguageProvider, useLocale } from "./LanguageProvider.js";
import { LANGUAGE_STORAGE_KEY, registerLocale, resetLocalesForTests } from "./locales.js";

const PROFILE_ID = "123e4567-e89b-12d3-a456-426614174000";
const USER_ID = "223e4567-e89b-12d3-a456-426614174001";

function profileFixture(language: string, user: string = USER_ID) {
  return {
    id: PROFILE_ID,
    user,
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
    language,
    start_of_the_week: 0,
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
}

function harness(language: string, patchStatus = 200) {
  const patches: Array<Record<string, unknown>> = [];
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
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patches.push(body);
      return {
        status: patchStatus,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          patchStatus === 200
            ? JSON.stringify(profileFixture(String(body["language"] ?? language)))
            : '{"detail":"boom"}',
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(profileFixture(language)),
    };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const notifyError = vi.fn();
  return { queryClient, notifyError, patches };
}

function LandingProbe({ landings }: { landings: Array<Promise<boolean>> }): React.ReactElement {
  const locale = useLocale();
  return (
    <button type="button" onClick={() => landings.push(locale.setLanguage("fr"))}>
      land french
    </button>
  );
}

function UnknownLandingProbe({ landings }: { landings: Array<Promise<boolean>> }): React.ReactElement {
  const locale = useLocale();
  return (
    <button type="button" onClick={() => landings.push(locale.setLanguage("xx"))}>
      land unknown
    </button>
  );
}

function TagProbe(): React.ReactElement {
  const locale = useLocale();
  return (
    <div>
      <span data-testid="tag">{locale.tag}</span>
      <span data-testid="supported">{locale.supported.map((entry) => entry.tag).join(",")}</span>
      <button type="button" onClick={() => locale.setLanguage("fr")}>
        pick french
      </button>
      <button type="button" onClick={() => locale.setLanguage("xx")}>
        pick unknown
      </button>
    </div>
  );
}

describe("LanguageProvider", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetAppClient();
    resetLocalesForTests();
    window.localStorage.clear();
    document.documentElement.removeAttribute("lang");
  });

  it("throws when useLocale renders without a provider", () => {
    expect(() => render(<TagProbe />)).toThrow("useLocale must render under LanguageProvider");
  });

  it("stamps the document, syncs the server tag once and re-renders subscribers", async () => {
    registerLocale({ tag: "fr", label: "French" });
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    const { queryClient, notifyError } = harness("fr");
    render(
      <QueryClientProvider client={queryClient}>
        <LanguageProvider notifyError={notifyError}>
          <TagProbe />
        </LanguageProvider>
      </QueryClientProvider>
    );
    // The stored choice wins over the server value on first sync.
    await waitFor(() => expect(screen.getByTestId("tag")).toHaveTextContent("en"));
    expect(document.documentElement.getAttribute("lang")).toBe("en");
    expect(screen.getByTestId("supported")).toHaveTextContent("en,fr");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("switches, persists twice and keeps the tag on failed writes", async () => {
    const user = userEvent.setup();
    registerLocale({ tag: "fr", label: "French" });
    const { queryClient, notifyError, patches } = harness("en");
    render(
      <QueryClientProvider client={queryClient}>
        <LanguageProvider notifyError={notifyError}>
          <TagProbe />
        </LanguageProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("tag")).toHaveTextContent("en"));
    await user.click(screen.getByRole("button", { name: "pick french" }));
    expect(screen.getByTestId("tag")).toHaveTextContent("fr");
    expect(document.documentElement.getAttribute("lang")).toBe("fr");
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("fr");
    await waitFor(() => expect(patches).toEqual([{ language: "fr" }]));
    await user.click(screen.getByRole("button", { name: "pick unknown" }));
    expect(notifyError).toHaveBeenCalledWith("Couldn't update the language", expect.any(String));
    expect(screen.getByTestId("tag")).toHaveTextContent("fr");
  });

  it("resolves true only once the profile write lands", async () => {
    const user = userEvent.setup();
    registerLocale({ tag: "fr", label: "French" });
    const { queryClient, notifyError } = harness("en");
    const landings: Array<Promise<boolean>> = [];
    render(
      <QueryClientProvider client={queryClient}>
        <LanguageProvider notifyError={notifyError}>
          <LandingProbe landings={landings} />
        </LanguageProvider>
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "land french" }));
    await expect(landings[0]).resolves.toBe(true);
  });

  it("resolves false for a failed write and for an unknown tag", async () => {
    const user = userEvent.setup();
    const { queryClient, notifyError } = harness("en", 500);
    const landings: Array<Promise<boolean>> = [];
    render(
      <QueryClientProvider client={queryClient}>
        <LanguageProvider notifyError={notifyError}>
          <LandingProbe landings={landings} />
          <UnknownLandingProbe landings={landings} />
        </LanguageProvider>
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "land unknown" }));
    await expect(landings[0]).resolves.toBe(false);
    await user.click(screen.getByRole("button", { name: "land french" }));
    await waitFor(() => expect(landings).toHaveLength(2));
    await expect(landings[1]).resolves.toBe(false);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the language", expect.any(String)));
  });

  it("toasts failed profile writes", async () => {
    const user = userEvent.setup();
    registerLocale({ tag: "fr", label: "French" });
    const { queryClient, notifyError } = harness("en", 500);
    render(
      <QueryClientProvider client={queryClient}>
        <LanguageProvider notifyError={notifyError}>
          <TagProbe />
        </LanguageProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("tag")).toHaveTextContent("en"));
    await user.click(screen.getByRole("button", { name: "pick french" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the language", expect.any(String)));
  });
});
