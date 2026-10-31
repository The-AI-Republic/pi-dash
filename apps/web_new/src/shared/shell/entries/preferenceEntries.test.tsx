// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette preference entries (shell spec, SHELL-088).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../toaster.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  LANGUAGE_STORAGE_KEY,
  LanguageProvider,
  registerLocale,
  resetLocalesForTests,
  useLocale,
} from "../../../core/i18n/index.js";
import { THEME_STORAGE_KEY, ThemeProvider } from "../../../core/theme/index.js";
import { useCommandPalette, type PalettePage } from "../../commands/index.js";
import { notifyError, notifySuccess } from "../toaster.js";
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

function stubClient(args: { bodies?: string[]; timezonesGate?: Promise<void>; timezonesStatus?: number } = {}) {
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
      const status = args.timezonesStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status === 200
            ? JSON.stringify({
                timezones: [
                  { utc_offset: "+01:00", gmt_offset: "GMT+1", value: "Europe/Paris", label: "Paris" },
                  { utc_offset: "+09:00", gmt_offset: "GMT+9", value: "Asia/Tokyo", label: "Tokyo" },
                ],
              })
            : JSON.stringify({ detail: "no" }),
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

function LocaleTagProbe(): React.ReactElement {
  const locale = useLocale();
  return <p data-testid="locale-tag">{locale.tag}</p>;
}

afterEach(() => {
  resetAppClient();
  resetLocalesForTests();
  useCommandPalette.getState().closePalette();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("lang");
  window.localStorage.clear();
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderPage(page: PalettePage, query: string) {
  render(
    <QueryClientProvider client={testClient()}>
      <ThemeProvider notifyError={vi.mocked(notifyError)}>
        <LanguageProvider notifyError={vi.mocked(notifyError)}>
          <LocaleTagProbe />
          <PageProbe page={page} query={query} />
        </LanguageProvider>
      </ThemeProvider>
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
  it("lists every engine option and applies the pick immediately without reloading", async () => {
    const bodies = stubClient();
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    useCommandPalette.getState().openPalette();
    renderPage(themePickerPage(), "");
    expect(themePickerPage().placeholder).toBe("Choose a theme");
    for (const label of [
      "System preference",
      "Light",
      "Dark",
      "Light high contrast",
      "Dark high contrast",
      "Custom theme",
    ]) {
      expect(screen.getByRole("button", { name: label })).not.toBeNull();
    }
    // Let the provider's one-time server sync land before picking (the
    // stub profile carries an empty bag, so the stored dark falls back
    // to following the OS); otherwise the sync would yank the pick back.
    await waitFor(() => expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("system"));
    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(useCommandPalette.getState().open).toBe(false);
    // Applied immediately, before the network settles; no reload.
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ theme: { theme: "dark" } });
    expect(notifySuccess).not.toHaveBeenCalled();
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
    expect(timezonePickerPage().placeholder).toBe("Change timezone");
    expect(screen.getByText("loading:No timezones found")).not.toBeNull();
    release();
    await waitFor(() => expect(screen.getByRole("button", { name: "Paris" })).not.toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Tokyo" }));
    expect(useCommandPalette.getState().open).toBe(false);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Timezone updated successfully."));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ user_timezone: "Asia/Tokyo" });
  });

  it("filters zones by label and value", async () => {
    stubClient();
    renderPage(timezonePickerPage(), "tok");
    await waitFor(() => expect(screen.getByRole("button", { name: "Tokyo" })).not.toBeNull());
    expect(screen.queryByRole("button", { name: "Paris" })).toBeNull();
  });

  it("renders the empty line when the zone list fails", async () => {
    stubClient({ timezonesStatus: 500 });
    renderPage(timezonePickerPage(), "");
    await waitFor(() => expect(screen.getByText("empty:No timezones found")).not.toBeNull());
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
  it("lists the engine offer and persists the pick through the engine", async () => {
    const bodies = stubClient();
    useCommandPalette.getState().openPalette();
    renderPage(languagePickerPage(), "");
    expect(languagePickerPage().placeholder).toBe("Choose a language");
    // The OSS build offers English only (SHELL-096).
    expect(screen.getByRole("button", { name: "English" })).not.toBeNull();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    await waitFor(() => expect(screen.getByTestId("locale-tag")).toHaveTextContent("en"));
    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(useCommandPalette.getState().open).toBe(false);
    // Engine effects land at once: document stamp, local persist, tag.
    expect(document.documentElement.getAttribute("lang")).toBe("en");
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(screen.getByTestId("locale-tag")).toHaveTextContent("en");
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ language: "en" });
    // The engine toasts failures only; a successful switch stays silent.
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("re-renders subscribers when a fuller build offers another locale", async () => {
    registerLocale({ tag: "fr", label: "French" });
    const bodies = stubClient();
    useCommandPalette.getState().openPalette();
    renderPage(languagePickerPage(), "");
    await waitFor(() => expect(screen.getByTestId("locale-tag")).toHaveTextContent("en"));
    fireEvent.click(screen.getByRole("button", { name: "French" }));
    expect(useCommandPalette.getState().open).toBe(false);
    expect(document.documentElement.getAttribute("lang")).toBe("fr");
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("fr");
    expect(screen.getByTestId("locale-tag")).toHaveTextContent("fr");
    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ language: "fr" });
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("filters languages", () => {
    registerLocale({ tag: "fr", label: "French" });
    stubClient();
    renderPage(languagePickerPage(), "fre");
    expect(screen.getByRole("button", { name: "French" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "English" })).toBeNull();
  });
});
