// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../api/client.js";
import { readDocumentScheme, ThemeProvider, useResolvedScheme, useTheme } from "./ThemeProvider.js";
import { THEME_PALETTE_STORAGE_KEY, THEME_STORAGE_KEY, writeStoredPalette, writeStoredTheme } from "./theme.js";

const PROFILE_ID = "123e4567-e89b-12d3-a456-426614174000";
const USER_ID = "223e4567-e89b-12d3-a456-426614174001";

function profileFixture(theme: Record<string, unknown>, user: string = USER_ID) {
  return {
    id: PROFILE_ID,
    user,
    theme,
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
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

interface HarnessOptions {
  profile?: Record<string, unknown>;
  patch?: (bag: Record<string, unknown>) => unknown;
}

function harness({ profile = { theme: "dark" }, patch }: HarnessOptions = {}) {
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
      const body = JSON.parse(init.body ?? "{}") as { theme?: Record<string, unknown> };
      patches.push(body.theme ?? {});
      const next = patch?.(body.theme ?? {}) ?? profileFixture({ ...(body.theme ?? {}) });
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(next) };
    }
    return {
      status: 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(profileFixture(profile)),
    };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const notifyError = vi.fn();
  return { queryClient, notifyError, patches };
}

function ValueProbe(): React.ReactElement {
  const theme = useTheme();
  return (
    <div>
      <span data-testid="value">{theme.value}</span>
      <span data-testid="resolved">{theme.resolved}</span>
      <button type="button" onClick={() => theme.setTheme("light")}>
        pick light
      </button>
      <button
        type="button"
        onClick={() => theme.setTheme("custom", { primary: "#112233", background: "#445566", darkPalette: true })}
      >
        pick custom
      </button>
      <button type="button" onClick={() => theme.setTheme("custom")}>
        re-pick custom
      </button>
    </div>
  );
}

function matchMediaStub(matches: boolean) {
  const listeners = new Set<(event: { matches: boolean }) => void>();
  const api = {
    matches,
    addEventListener: (_event: string, listener: (event: { matches: boolean }) => void) => listeners.add(listener),
    removeEventListener: (_event: string, listener: (event: { matches: boolean }) => void) =>
      listeners.delete(listener),
    dispatch: () => listeners.forEach((listener) => listener({ matches: api.matches })),
  };
  vi.stubGlobal("matchMedia", () => api);
  return api;
}

describe("ThemeProvider", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetAppClient();
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  it("throws when useTheme renders without a provider", () => {
    expect(() => render(<ValueProbe />)).toThrow("useTheme must render under ThemeProvider");
  });

  it("reads the resolved scheme from the provider or the document mark", async () => {
    function SchemeProbe(): React.ReactElement {
      const scheme = useResolvedScheme();
      return <span data-testid="scheme">{`${scheme.scheme}:${scheme.customDark}`}</span>;
    }
    expect(readDocumentScheme("dark")).toBe("dark");
    expect(readDocumentScheme("dark-contrast")).toBe("dark-contrast");
    expect(readDocumentScheme("light-contrast")).toBe("light-contrast");
    expect(readDocumentScheme("custom")).toBe("custom");
    expect(readDocumentScheme("light")).toBe("light");
    expect(readDocumentScheme("sepia")).toBe("light");
    expect(readDocumentScheme(null)).toBe("light");
    document.documentElement.setAttribute("data-theme", "dark-contrast");
    const { unmount } = render(<SchemeProbe />);
    expect(screen.getByTestId("scheme")).toHaveTextContent("dark-contrast:false");
    unmount();
    matchMediaStub(false);
    const { queryClient } = harness({ profile: { theme: "dark" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <SchemeProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("scheme")).toHaveTextContent("dark:false"));
  });

  it("reads a dark custom palette without a provider", () => {
    function SchemeProbe(): React.ReactElement {
      const scheme = useResolvedScheme();
      return <span data-testid="scheme">{`${scheme.scheme}:${scheme.customDark}`}</span>;
    }
    window.localStorage.setItem(
      THEME_PALETTE_STORAGE_KEY,
      JSON.stringify({ primary: "#112233", background: "#445566", darkPalette: true })
    );
    document.documentElement.setAttribute("data-theme", "custom");
    render(<SchemeProbe />);
    expect(screen.getByTestId("scheme")).toHaveTextContent("custom:true");
  });

  it("reports a dark custom base through the resolved scheme", async () => {
    function SchemeProbe(): React.ReactElement {
      const scheme = useResolvedScheme();
      return <span data-testid="scheme">{`${scheme.scheme}:${scheme.customDark}`}</span>;
    }
    matchMediaStub(false);
    const { queryClient } = harness({
      profile: { theme: "custom", primary: "#112233", background: "#445566", darkPalette: true },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <SchemeProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("scheme")).toHaveTextContent("custom:true"));
  });

  it("applies the stored choice, then syncs the server value once per user", async () => {
    matchMediaStub(false);
    window.localStorage.setItem(THEME_STORAGE_KEY, "light");
    const { queryClient, notifyError } = harness({ profile: { theme: "dark" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={notifyError}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("dark"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("persists picks locally and on the profile at once", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const { queryClient, patches } = harness({ profile: { theme: "system" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("system"));
    await user.click(screen.getByRole("button", { name: "pick light" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    await waitFor(() => expect(patches).toEqual([{ theme: "light" }]));
  });

  it("merges picks over the cached bag and sends a bare bag before the profile lands", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const cached = { theme: "custom", primary: "#a", background: "#b", darkPalette: true };
    const merged = harness({ profile: cached });
    const first = render(
      <QueryClientProvider client={merged.queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("custom"));
    await user.click(screen.getByRole("button", { name: "pick light" }));
    await waitFor(() => expect(merged.patches).toEqual([{ ...cached, theme: "light" }]));
    first.unmount();

    // Before the profile lands there is nothing to merge over.
    const bare = harness({ profile: { theme: "system" } });
    render(
      <QueryClientProvider client={bare.queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "pick light" }));
    await waitFor(() => expect(bare.patches).toEqual([{ theme: "light" }]));
  });

  it("tracks the OS while system holds and stops after leaving it", async () => {
    const user = userEvent.setup();
    const media = matchMediaStub(false);
    const { queryClient } = harness({ profile: { theme: "system" } });
    const { unmount } = render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("resolved")).toHaveTextContent("light"));
    (media as unknown as { matches: boolean }).matches = true;
    media.dispatch();
    await waitFor(() => expect(document.documentElement.getAttribute("data-theme")).toBe("dark"));
    await user.click(screen.getByRole("button", { name: "pick light" }));
    (media as unknown as { matches: boolean }).matches = false;
    media.dispatch();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    unmount();
  });

  it("renders without OS tracking where matchMedia is missing", async () => {
    // No matchMedia stub: hosts without it (some test shells) skip the
    // watcher instead of crashing; system resolves light.
    const { queryClient } = harness({ profile: { theme: "system" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("resolved")).toHaveTextContent("light"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("applies custom palettes with their variables", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const { queryClient, patches } = harness({ profile: { theme: "light" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("light"));
    await user.click(screen.getByRole("button", { name: "pick custom" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("custom");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("#112233");
    await waitFor(() =>
      expect(patches).toEqual([{ theme: "custom", primary: "#112233", background: "#445566", darkPalette: true }])
    );
  });

  it("rolls the switcher and the document back when the write fails", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const transport = async (url: string, init: { method: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => '{"csrf_token":"csrf"}',
        };
      }
      if (init.method === "PATCH") {
        return {
          status: 500,
          url,
          headers: { get: () => "application/json" },
          text: async () => '{"detail":"boom"}',
        };
      }
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(profileFixture({ theme: "dark" })),
      };
    };
    createAppClient("https://api.test", transport as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const notifyError = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={notifyError}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("dark"));
    await user.click(screen.getByRole("button", { name: "pick light" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the theme", expect.any(String)));
    // NEWFRONT-434 target: the document rolls back with the switcher.
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("dark"));
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
  });

  it("drops a rejected custom palette from local storage", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const transport = async (url: string, init: { method: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => '{"csrf_token":"csrf"}',
        };
      }
      if (init.method === "PATCH") {
        return {
          status: 500,
          url,
          headers: { get: () => "application/json" },
          text: async () => '{"detail":"boom"}',
        };
      }
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(profileFixture({ theme: "light" })),
      };
    };
    createAppClient("https://api.test", transport as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("light"));
    await user.click(screen.getByRole("button", { name: "pick custom" }));
    await waitFor(() => expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light"));
    // The next boot must not paint the refused colors.
    expect(window.localStorage.getItem(THEME_PALETTE_STORAGE_KEY)).toBeNull();
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("");
  });

  it("restores the accepted palette when a later custom save is rejected", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const accepted = { primary: "#102030", background: "#405060", darkPalette: true };
    let patches = 0;
    const { queryClient } = harness({
      profile: { theme: "custom", ...accepted },
      patch: () => {
        patches += 1;
        // The new palette is refused; the accepted one stays in place.
        throw new Error("boom");
      },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("custom"));
    await waitFor(() => expect(window.localStorage.getItem(THEME_PALETTE_STORAGE_KEY)).toBe(JSON.stringify(accepted)));
    await user.click(screen.getByRole("button", { name: "pick custom" }));
    await waitFor(() => expect(patches).toBe(1));
    await waitFor(() => expect(window.localStorage.getItem(THEME_PALETTE_STORAGE_KEY)).toBe(JSON.stringify(accepted)));
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("#102030");
  });

  it("re-applies the saved custom palette when custom is picked again", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const { queryClient, patches } = harness({
      profile: { theme: "custom", primary: "#102030", background: "#405060", darkPalette: true },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("custom"));
    await user.click(screen.getByRole("button", { name: "pick light" }));
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("");
    await user.click(screen.getByRole("button", { name: "re-pick custom" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("custom");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("#102030");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-background")).toBe("#405060");
    await waitFor(() =>
      expect(patches).toEqual([
        { theme: "light", primary: "#102030", background: "#405060", darkPalette: true },
        { theme: "custom", primary: "#102030", background: "#405060", darkPalette: true },
      ])
    );
  });

  it("sends only the name when custom is picked with no saved palette", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const { queryClient, patches } = harness({ profile: { theme: "light" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("light"));
    await user.click(screen.getByRole("button", { name: "re-pick custom" }));
    expect(document.documentElement.getAttribute("data-theme")).toBe("custom");
    // The designer's fallback colors are a starting point, not a save.
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("");
    await waitFor(() => expect(patches).toEqual([{ theme: "custom" }]));
    expect(window.localStorage.getItem(THEME_PALETTE_STORAGE_KEY)).toBeNull();
  });

  it("reports whether the profile write landed", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const { queryClient } = harness({ profile: { theme: "light" } });
    const landings: Array<Promise<boolean>> = [];
    function LandingProbe(): React.ReactElement {
      const theme = useTheme();
      return (
        <button
          type="button"
          onClick={() => {
            landings.push(theme.setTheme("dark"));
          }}
        >
          pick dark
        </button>
      );
    }
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <LandingProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "pick dark" }));
    expect(landings).toHaveLength(1);
    await expect(landings[0]).resolves.toBe(true);
  });

  it("reports a rejected write instead of leaving the caller waiting", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const transport = async (url: string, init: { method: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => '{"csrf_token":"csrf"}',
        };
      }
      if (init.method === "PATCH") {
        return { status: 500, url, headers: { get: () => "application/json" }, text: async () => '{"detail":"boom"}' };
      }
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(profileFixture({ theme: "light" })),
      };
    };
    createAppClient("https://api.test", transport as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    const landings: Array<Promise<boolean>> = [];
    function LandingProbe(): React.ReactElement {
      const theme = useTheme();
      return (
        <button
          type="button"
          onClick={() => {
            landings.push(theme.setTheme("dark"));
          }}
        >
          pick dark
        </button>
      );
    }
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <LandingProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "pick dark" }));
    await expect(landings[0]).resolves.toBe(false);
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("stores the picked palette beside the choice", async () => {
    const user = userEvent.setup();
    matchMediaStub(false);
    const { queryClient } = harness({ profile: { theme: "light" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("value")).toHaveTextContent("light"));
    await user.click(screen.getByRole("button", { name: "pick custom" }));
    expect(window.localStorage.getItem(THEME_PALETTE_STORAGE_KEY)).toBe(
      JSON.stringify({ primary: "#112233", background: "#445566", darkPalette: true })
    );
  });

  it("boots the stored custom palette before the profile lands", async () => {
    matchMediaStub(false);
    writeStoredTheme("custom");
    writeStoredPalette({ primary: "#0a0b0c", background: "#fefefe", darkPalette: false });
    const { queryClient } = harness({ profile: { theme: "custom", primary: "#0a0b0c", background: "#fefefe" } });
    render(
      <QueryClientProvider client={queryClient}>
        <ThemeProvider notifyError={() => undefined}>
          <ValueProbe />
        </ThemeProvider>
      </QueryClientProvider>
    );
    expect(screen.getByTestId("value")).toHaveTextContent("custom");
    expect(document.documentElement.style.getPropertyValue("--theme-custom-primary")).toBe("#0a0b0c");
    await waitFor(() => expect(screen.getByTestId("resolved")).toHaveTextContent("custom"));
  });
});
