// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./toaster.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient, getAppClient } from "../../core/api/client.js";
import { sessionKeys } from "../../core/session/queries.js";
import { notifyError, notifySuccess } from "./toaster.js";
import {
  FIRST_DAY_CHOICES,
  LANGUAGE_CHOICES,
  PREFERENCES_FAILED_TITLE,
  PREFERENCES_UPDATED_TITLE,
  THEME_CHOICES,
  THEME_FAILED_TITLE,
  THEME_UPDATED_MESSAGE,
  THEME_UPDATED_TITLE,
  TIMEZONE_FAILED_TITLE,
  TIMEZONE_UPDATED_TITLE,
  reloadApp,
  timezonesQueryOptions,
  useUpdateFirstDay,
  useUpdateLanguage,
  useUpdateTheme,
  useUpdateTimezone,
} from "./preferences.js";

const USER_ID = "123e4567-e89b-12d3-a456-426614174000";

function profileBody(overrides: Record<string, unknown> = {}) {
  return {
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
    ...overrides,
  };
}

interface StubArgs {
  profileStatus?: number;
  meStatus?: number;
  timezonesStatus?: number;
  bodies?: string[];
}

function stubClient(args: StubArgs = {}) {
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
      const status = args.profileStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(profileBody()) : JSON.stringify({ detail: "no" })),
      };
    }
    if (url.endsWith("/api/users/me/")) {
      const status = args.meStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status === 200
            ? JSON.stringify({ id: USER_ID, email: "ada@example.com", user_timezone: "Europe/Paris" })
            : JSON.stringify({ detail: "no" }),
      };
    }
    if (url.endsWith("/api/timezones/")) {
      const status = args.timezonesStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status === 200
            ? JSON.stringify({
                timezones: [{ utc_offset: "+01:00", gmt_offset: "GMT+1", value: "Europe/Paris", label: "Paris" }],
              })
            : JSON.stringify({ detail: "no" }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return bodies;
}

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

afterEach(() => {
  resetAppClient();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.lang = "";
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

describe("preference option lists", () => {
  it("offers the engine-supported themes, seven days and OSS English", () => {
    expect(THEME_CHOICES).toEqual([
      { value: "system", label: "System preference" },
      { value: "light", label: "Light" },
      { value: "dark", label: "Dark" },
    ]);
    expect(FIRST_DAY_CHOICES.map((choice) => choice.label)).toEqual([
      "Sunday",
      "Monday",
      "Tuesday",
      "Wednesday",
      "Thursday",
      "Friday",
      "Saturday",
    ]);
    // Sunday is zero on the server (SHELL-088).
    expect(FIRST_DAY_CHOICES.map((choice) => choice.value)).toEqual([0, 1, 2, 3, 4, 5, 6]);
    expect(LANGUAGE_CHOICES).toEqual([{ value: "en", label: "English" }]);
  });
});

describe("useUpdateTheme", () => {
  function ThemeProbe({ reload }: { reload: () => void }) {
    const update = useUpdateTheme({ reload });
    const [outcome, setOutcome] = React.useState("idle");
    return (
      <button
        type="button"
        onClick={() => {
          update.mutate({ theme: "dark" }, { onError: () => setOutcome("failed"), onSuccess: () => setOutcome("ok") });
        }}
      >
        outcome:{outcome}
      </button>
    );
  }

  it("applies at once, persists the merged bag, toasts and reloads", async () => {
    const bodies = stubClient();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.profile, profileBody({ theme: { theme: "light", primary: "#fff" } }));
    const reload = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProbe reload={reload} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:ok"));
    // Applied immediately, before the network settles.
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(bodies).toHaveLength(1);
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({
      theme: { theme: "dark", primary: "#fff" },
    });
    expect(notifySuccess).toHaveBeenCalledWith(THEME_UPDATED_TITLE, THEME_UPDATED_MESSAGE);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("sends a bare theme bag when no profile is cached", async () => {
    const bodies = stubClient();
    const reload = vi.fn();
    render(
      <QueryClientProvider client={testClient()}>
        <ThemeProbe reload={reload} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:ok"));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ theme: { theme: "dark" } });
  });

  it("toasts and reports failure without reloading", async () => {
    stubClient({ profileStatus: 500 });
    const reload = vi.fn();
    render(
      <QueryClientProvider client={testClient()}>
        <ThemeProbe reload={reload} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:failed"));
    expect(notifyError).toHaveBeenCalledWith(THEME_FAILED_TITLE, expect.any(String));
    expect(reload).not.toHaveBeenCalled();
  });
});

describe("useUpdateFirstDay", () => {
  function DayProbe() {
    const update = useUpdateFirstDay();
    const [outcome, setOutcome] = React.useState("idle");
    return (
      <button
        type="button"
        onClick={() => {
          update.mutate({ startOfWeek: 1 }, { onError: () => setOutcome("failed"), onSuccess: () => setOutcome("ok") });
        }}
      >
        outcome:{outcome}
      </button>
    );
  }

  it("persists Monday as 1 with a confirmation toast", async () => {
    const bodies = stubClient();
    render(
      <QueryClientProvider client={testClient()}>
        <DayProbe />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:ok"));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ start_of_the_week: 1 });
    expect(notifySuccess).toHaveBeenCalledWith(PREFERENCES_UPDATED_TITLE);
  });

  it("toasts failures without persisting", async () => {
    stubClient({ profileStatus: 500 });
    render(
      <QueryClientProvider client={testClient()}>
        <DayProbe />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:failed"));
    expect(notifyError).toHaveBeenCalledWith(PREFERENCES_FAILED_TITLE, expect.any(String));
  });
});

describe("useUpdateLanguage", () => {
  function LanguageProbe({ failing }: { failing: boolean }) {
    const update = useUpdateLanguage();
    const [outcome, setOutcome] = React.useState("idle");
    return (
      <button
        type="button"
        onClick={() => {
          update.mutate({ language: "en" }, { onError: () => setOutcome("failed"), onSuccess: () => setOutcome("ok") });
        }}
      >
        outcome:{outcome}:{failing ? "x" : "y"}
      </button>
    );
  }

  it("persists the language, flips the document language and toasts", async () => {
    const bodies = stubClient();
    render(
      <QueryClientProvider client={testClient()}>
        <LanguageProbe failing={false} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:ok"));
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ language: "en" });
    expect(document.documentElement.lang).toBe("en");
    expect(notifySuccess).toHaveBeenCalledWith(PREFERENCES_UPDATED_TITLE);
  });

  it("toasts language failures", async () => {
    stubClient({ profileStatus: 500 });
    render(
      <QueryClientProvider client={testClient()}>
        <LanguageProbe failing />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:failed"));
    expect(notifyError).toHaveBeenCalledWith(PREFERENCES_FAILED_TITLE, expect.any(String));
  });
});

describe("useUpdateTimezone", () => {
  function TimezoneProbe() {
    const update = useUpdateTimezone();
    const [outcome, setOutcome] = React.useState("idle");
    return (
      <button
        type="button"
        onClick={() => {
          update.mutate(
            { timezone: "Europe/Paris" },
            { onError: () => setOutcome("failed"), onSuccess: () => setOutcome("ok") }
          );
        }}
      >
        outcome:{outcome}
      </button>
    );
  }

  it("persists the timezone to the user row with a confirmation toast", async () => {
    const bodies = stubClient();
    render(
      <QueryClientProvider client={testClient()}>
        <TimezoneProbe />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:ok"));
    expect(bodies[0]).toContain("PATCH https://api.test/api/users/me/");
    expect(JSON.parse(bodies[0]!.split(" ").slice(2).join(" "))).toEqual({ user_timezone: "Europe/Paris" });
    expect(notifySuccess).toHaveBeenCalledWith(TIMEZONE_UPDATED_TITLE);
  });

  it("toasts timezone failures", async () => {
    stubClient({ meStatus: 500 });
    render(
      <QueryClientProvider client={testClient()}>
        <TimezoneProbe />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("outcome:failed"));
    expect(notifyError).toHaveBeenCalledWith(TIMEZONE_FAILED_TITLE, expect.any(String));
  });
});

describe("timezones query", () => {
  function ZonesProbe() {
    const zones = useQuery(timezonesQueryOptions(getAppClient()));
    if (zones.isPending) return <p>loading</p>;
    if (zones.isError) return <p>failed</p>;
    return <p>zones:{zones.data.timezones.map((zone) => zone.value).join(",")}</p>;
  }

  it("reads the zone list", async () => {
    stubClient();
    render(
      <QueryClientProvider client={testClient()}>
        <ZonesProbe />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByText("zones:Europe/Paris")).not.toBeNull());
  });
});

describe("reloadApp", () => {
  it("reloads the page", () => {
    const reload = vi.fn();
    const location = window.location;
    Object.defineProperty(window, "location", { value: { ...location, reload }, writable: true });
    try {
      reloadApp();
      expect(reload).toHaveBeenCalledTimes(1);
    } finally {
      Object.defineProperty(window, "location", { value: location, writable: true });
    }
  });
});
