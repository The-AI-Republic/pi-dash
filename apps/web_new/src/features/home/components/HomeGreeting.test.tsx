// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { sessionKeys, setSessionClient } from "../../../core/session/queries.js";
import { HomeGreeting, useNow } from "./HomeGreeting.js";

function meRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    email: "ada@example.com",
    username: "ada",
    first_name: "Ada",
    last_name: "Lovelace",
    display_name: "Ada Lovelace",
    user_timezone: "UTC",
    ...overrides,
  };
}

function renderGreeting(options: {
  me?: Record<string, unknown> | undefined;
  language?: string;
  seedProfile?: boolean;
}): void {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.me !== undefined) queryClient.setQueryData(sessionKeys.me, options.me);
  if (options.seedProfile !== false) {
    queryClient.setQueryData(sessionKeys.profile, {
      id: "223e4567-e89b-12d3-a456-426614174001",
      language: options.language ?? "en",
      is_tour_completed: true,
    });
  }
  render(
    <QueryClientProvider client={queryClient}>
      <HomeGreeting />
    </QueryClientProvider>
  );
}

afterEach(() => {
  vi.useRealTimers();
});

describe("HomeGreeting", () => {
  it("greets the user with the time-of-day variant", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-14T09:15:00Z"));
    renderGreeting({ me: meRow() });
    expect(screen.getByRole("heading", { name: "Good morning, Ada" })).toBeInTheDocument();
    expect(screen.getByText("Wednesday, Oct 14 9:15 AM")).toBeInTheDocument();
  });

  it("follows the evening variant in the user's timezone", () => {
    vi.useFakeTimers();
    // 22:15 UTC is 18:15 in New York: evening there, and the clock agrees.
    vi.setSystemTime(new Date("2026-10-14T22:15:00Z"));
    renderGreeting({ me: meRow({ user_timezone: "America/New_York" }) });
    expect(screen.getByRole("heading", { name: "Good evening, Ada" })).toBeInTheDocument();
    expect(screen.getByText("Wednesday, Oct 14 6:15 PM")).toBeInTheDocument();
  });

  it("advances the clock without a reload", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-14T09:15:30Z"));
    renderGreeting({ me: meRow() });
    expect(screen.getByText("Wednesday, Oct 14 9:15 AM")).toBeInTheDocument();
    vi.setSystemTime(new Date("2026-10-14T09:16:05Z"));
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByText("Wednesday, Oct 14 9:16 AM")).toBeInTheDocument();
  });

  it("keeps the English salutation on unknown and malformed locales", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-14T09:15:00Z"));
    renderGreeting({ me: meRow(), language: "de" });
    expect(screen.getByRole("heading", { name: "Good morning, Ada" })).toBeInTheDocument();
  });

  it("greets plainly without a name and degrades a broken timezone", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-14T09:15:00Z"));
    renderGreeting({ me: meRow({ first_name: "", user_timezone: "Mars/Olympus_Mons" }), language: "xx!!" });
    const heading = screen.getByRole("heading");
    expect(heading.textContent ?? "").toContain("morning");
    expect(heading.textContent ?? "").not.toContain(",");
    expect(screen.getByText(/[A-Za-z]+, [A-Za-z]{3} \d{1,2} \d{1,2}:\d{2}/)).toBeInTheDocument();
  });

  it("falls back to English while the profile loads", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-14T09:15:00Z"));
    renderGreeting({ me: meRow(), seedProfile: false });
    expect(screen.getByRole("heading", { name: "Good morning, Ada" })).toBeInTheDocument();
  });

  it("renders nothing while the user loads", () => {
    renderGreeting({});
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
  });
});

describe("useNow", () => {
  it("clears its ticker on unmount", () => {
    vi.useFakeTimers();
    const clearSpy = vi.spyOn(globalThis, "clearInterval");
    try {
      setSessionClient({
        getJson: () => new Promise<never>(() => {}),
        parse: (_schema: unknown, data: unknown) => data,
      } as never);
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      function Probe(): React.ReactElement {
        useNow(5000);
        return <div />;
      }
      const { unmount } = render(
        <QueryClientProvider client={queryClient}>
          <Probe />
        </QueryClientProvider>
      );
      expect(clearSpy).not.toHaveBeenCalled();
      unmount();
      expect(clearSpy).toHaveBeenCalledTimes(1);
    } finally {
      clearSpy.mockRestore();
    }
  });
});
