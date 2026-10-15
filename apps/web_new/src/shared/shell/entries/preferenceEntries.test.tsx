// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette preference entries (shell spec, SHELL-088).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../toaster.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useCommandPalette, type PalettePage } from "../../commands/index.js";
import { notifySuccess } from "../toaster.js";
import {
  FIRST_DAY_PAGE_ID,
  LANGUAGE_PAGE_ID,
  THEME_PAGE_ID,
  TIMEZONE_PAGE_ID,
  buildPreferenceCommands,
  firstDayPickerPage,
  languagePickerPage,
  themePickerPage,
  timezonePickerPage,
} from "./preferenceEntries.js";

const USER_ID = "123e4567-e89b-12d3-a456-426614174000";

function stubClient(args: { bodies?: string[]; timezonesGate?: Promise<void> } = {}) {
  const bodies = args.bodies ?? [];
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: unknown }) => {
    if (typeof init?.body === "string") bodies.push(`${init?.method ?? "GET"} ${url} ${init.body}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/api/users/me/profile/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            id: USER_ID,
            user: USER_ID,
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
          }),
      };
    }
    if (url.endsWith("/api/users/me/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ id: USER_ID, email: "ada@example.com", user_timezone: "Europe/Paris" }),
      };
    }
    if (url.endsWith("/api/timezones/")) {
      if (args.timezonesGate) await args.timezonesGate;
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            timezones: [
              { utc_offset: "+01:00", gmt_offset: "GMT+1", value: "Europe/Paris", label: "Paris" },
              { utc_offset: "+09:00", gmt_offset: "GMT+9", value: "Asia/Tokyo", label: "Tokyo" },
            ],
          }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return bodies;
}

function PageProbe({ page, query }: { page: PalettePage; query: string }) {
  const state = page.usePageState(query);
  if (state.status === "loading") return <p>loading:{state.emptyText}</p>;
  return (
    <ul>
      {state.options.length === 0 ? <li>empty:{state.emptyText}</li> : null}
      {state.options.map((option) => (
        <li key={option.id}>
          <button type="button" onClick={() => void option.run()}>
            {option.title}
          </button>
        </li>
      ))}
    </ul>
  );
}

const reload = vi.fn();

beforeEach(() => {
  const location = window.location;
  Object.defineProperty(window, "location", { value: { ...location, reload }, writable: true, configurable: true });
});

afterEach(() => {
  resetAppClient();
  useCommandPalette.getState().closePalette();
  document.documentElement.removeAttribute("data-theme");
  vi.mocked(notifySuccess).mockClear();
  reload.mockClear();
});

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderPage(page: PalettePage, query: string) {
  render(
    <QueryClientProvider client={testClient()}>
      <PageProbe page={page} query={query} />
    </QueryClientProvider>
  );
}

describe("preference entries", () => {
  it("lists the four preference commands on their sub-pages", () => {
    expect(buildPreferenceCommands().map((command) => [command.title, command.section, command.page])).toEqual([
      ["Change interface theme", "Preferences", THEME_PAGE_ID],
      ["Change timezone", "Preferences", TIMEZONE_PAGE_ID],
      ["Change first day of week", "Preferences", FIRST_DAY_PAGE_ID],
      ["Change interface language", "Preferences", LANGUAGE_PAGE_ID],
    ]);
  });
});

describe("theme picker page", () => {
  it("applies, persists and reloads on pick", async () => {
    const bodies = stubClient();
    useCommandPalette.getState().openPalette();
    renderPage(themePickerPage(), "");
    expect(themePickerPage().placeholder).toBe("Choose a theme");
    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(useCommandPalette.getState().open).toBe(false);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Theme updated", expect.any(String)));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ theme: { theme: "dark" } });
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("filters to the empty line", () => {
    stubClient();
    renderPage(themePickerPage(), "nope");
    expect(screen.getByText("empty:No matching options.")).not.toBeNull();
  });
});

describe("timezone picker page", () => {
  it("spins, lists zones and persists on pick", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const bodies = stubClient({ timezonesGate: gate });
    useCommandPalette.getState().openPalette();
    renderPage(timezonePickerPage(), "");
    expect(timezonePickerPage().placeholder).toBe("Search timezones");
    expect(screen.getByText("loading:No timezones found")).not.toBeNull();
    release();
    await waitFor(() => expect(screen.getByRole("button", { name: "Paris" })).not.toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Tokyo" }));
    expect(useCommandPalette.getState().open).toBe(false);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Timezone updated"));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ user_timezone: "Asia/Tokyo" });
  });

  it("filters zones by label and value", async () => {
    stubClient();
    renderPage(timezonePickerPage(), "tok");
    await waitFor(() => expect(screen.getByRole("button", { name: "Tokyo" })).not.toBeNull());
    expect(screen.queryByRole("button", { name: "Paris" })).toBeNull();
  });
});

describe("first-day picker page", () => {
  it("persists Monday as 1 on pick", async () => {
    const bodies = stubClient();
    useCommandPalette.getState().openPalette();
    renderPage(firstDayPickerPage(), "");
    expect(firstDayPickerPage().placeholder).toBe("Choose the first day of the week");
    fireEvent.click(screen.getByRole("button", { name: "Monday" }));
    expect(useCommandPalette.getState().open).toBe(false);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Preferences updated"));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ start_of_the_week: 1 });
  });

  it("filters days", () => {
    stubClient();
    renderPage(firstDayPickerPage(), "sun");
    expect(screen.getByRole("button", { name: "Sunday" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Monday" })).toBeNull();
  });
});

describe("language picker page", () => {
  it("persists the language on pick", async () => {
    const bodies = stubClient();
    useCommandPalette.getState().openPalette();
    renderPage(languagePickerPage(), "");
    expect(languagePickerPage().placeholder).toBe("Choose a language");
    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(useCommandPalette.getState().open).toBe(false);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Preferences updated"));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ language: "en" });
  });
});
