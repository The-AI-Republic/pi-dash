// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Profile, Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { updateProfileMock } = vi.hoisted(() => ({ updateProfileMock: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, updateProfile: updateProfileMock };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifyInfo: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyInfo, notifySuccess } from "../../../shared/shell/index.js";
import { AGENT_TOOLS_LABEL } from "./AppsToggle.js";
import {
  APPS_EMPTY_DESCRIPTION,
  APPS_EMPTY_TITLE,
  APPS_NO_MATCHES_TITLE,
  APPS_SEARCH_PLACEHOLDER,
  AppsCatalog,
} from "./AppsCatalog.js";

function profileJson(enabled: boolean): Record<string, unknown> {
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
    settings: { openhub: { apps_enabled: enabled } },
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
  };
}

function appJson(appId: string, name: string): Record<string, unknown> {
  return { appId, slug: appId, name };
}

function pageJson(items: Array<Record<string, unknown>>, nextCursor: string | null = null): Record<string, unknown> {
  return { items, nextCursor };
}

interface CatalogCall {
  method: string;
  url: string;
}

interface CatalogTransportOptions {
  marketplace?: (
    params: URLSearchParams,
    call: number
  ) => { status: number; body: unknown } | Promise<{ status: number; body: unknown }>;
  profile?: Record<string, unknown> | "hang";
  install?: (appId: string) => { status: number; body: unknown };
  uninstall?: (appId: string) => { status: number; body: unknown };
  authStatus?: (appId: string) => { status: number; body: unknown };
  apiKey?: (appId: string) => { status: number; body: unknown };
}

function catalogTransport(options: CatalogTransportOptions = {}): { calls: CatalogCall[]; transport: Transport } {
  const calls: CatalogCall[] = [];
  let marketCalls = 0;
  const transport: Transport = async (url, init) => {
    calls.push({ method: init.method, url });
    const parsed = new URL(url);
    const answer = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (parsed.pathname === "/auth/get-csrf-token/") {
      return answer(200, { csrf_token: "test-csrf-token" });
    }
    if (parsed.pathname === "/api/openhub/apps/marketplace/") {
      marketCalls += 1;
      const hit = (await options.marketplace?.(parsed.searchParams, marketCalls)) ?? {
        status: 200,
        body: pageJson([]),
      };
      return answer(hit.status, hit.body);
    }
    if (parsed.pathname === "/api/users/me/profile/" && init.method === "GET") {
      if (options.profile === "hang") return new Promise<never>(() => {});
      return answer(200, options.profile ?? profileJson(false));
    }
    const install = /^\/api\/openhub\/apps\/([^/]+)\/install\/$/.exec(parsed.pathname);
    if (install !== null) {
      const appId = install[1] ?? "";
      const hit =
        init.method === "POST"
          ? (options.install?.(appId) ?? { status: 200, body: { installed: true } })
          : (options.uninstall?.(appId) ?? { status: 200, body: { installed: false } });
      return answer(hit.status, hit.body);
    }
    const auth = /^\/api\/openhub\/apps\/([^/]+)\/auth\/status\/$/.exec(parsed.pathname);
    if (auth !== null) {
      const hit = options.authStatus?.(auth[1] ?? "") ?? { status: 200, body: { type: "none", status: "active" } };
      return answer(hit.status, hit.body);
    }
    const key = /^\/api\/openhub\/apps\/([^/]+)\/auth\/api-key\/$/.exec(parsed.pathname);
    if (key !== null) {
      const hit = options.apiKey?.(key[1] ?? "") ?? { status: 200, body: { connected: true } };
      return answer(hit.status, hit.body);
    }
    throw new Error(`unexpected request ${init.method} ${url}`);
  };
  return { calls, transport };
}

function marketParams(calls: CatalogCall[]): URLSearchParams[] {
  return calls
    .filter((call) => call.url.includes("/api/openhub/apps/marketplace/"))
    .map((call) => new URL(call.url).searchParams);
}

function renderCatalog(transport: Transport): QueryClient {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AppsCatalog workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return queryClient;
}

function cardNames(): string[] {
  const grid = screen.getByTestId("apps-catalog-grid");
  return within(grid)
    .getAllByTestId("app-card-name")
    .map((name) => name.textContent ?? "");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("AppsCatalog", () => {
  it("mounts the catalog chrome with the viewer's apps (CLOUD-022)", async () => {
    const { calls, transport } = catalogTransport({
      marketplace: () => ({
        status: 200,
        body: pageJson([appJson("app-notes", "Notes Pro"), appJson("app-crm", "CRM Thing")]),
      }),
    });
    renderCatalog(transport);

    expect(await screen.findByRole("heading", { name: "Apps", level: 1 })).toBeInTheDocument();
    const intro = screen.getByRole("heading", { name: "Apps", level: 1 }).nextElementSibling?.textContent ?? "";
    expect(intro).toContain("yours");
    expect(intro).toContain("teammates");
    expect(screen.getByRole("searchbox")).toHaveAttribute("placeholder", APPS_SEARCH_PLACEHOLDER);
    expect(await screen.findByRole("switch", { name: AGENT_TOOLS_LABEL })).toHaveAttribute("aria-checked", "false");
    expect(cardNames()).toEqual(["Notes Pro", "CRM Thing"]);
    // Single window: no pager.
    expect(screen.queryByRole("navigation", { name: "Catalog pages" })).not.toBeInTheDocument();
    // The initial read carries only the page size.
    const initial = marketParams(calls);
    expect(initial).toHaveLength(1);
    expect(initial[0]?.get("limit")).toBe("24");
    expect(initial[0]?.get("q")).toBeNull();
    expect(initial[0]?.get("cursor")).toBeNull();
  });

  it("filters once per settled query and trims it (CLOUD-023)", async () => {
    const { calls, transport } = catalogTransport({
      marketplace: (params) => {
        const query = params.get("q") ?? "";
        if (query === "") {
          return {
            status: 200,
            body: pageJson([appJson("app-notes", "Notes Pro"), appJson("app-crm", "CRM Thing")]),
          };
        }
        return { status: 200, body: pageJson([appJson("app-notes", "Notes Pro")]) };
      },
    });
    renderCatalog(transport);
    expect(await screen.findByText("CRM Thing")).toBeInTheDocument();

    // Five keystrokes inside one debounce window: one request.
    const user = userEvent.setup();
    await user.type(screen.getByRole("searchbox"), "notes");
    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("CRM Thing")).not.toBeInTheDocument());
    expect(marketParams(calls).filter((params) => params.get("q") === "notes")).toHaveLength(1);
    await new Promise((resolve) => setTimeout(resolve, 550));
    expect(marketParams(calls).filter((params) => params.get("q") === "notes")).toHaveLength(1);

    // Surrounding whitespace never reaches the network.
    await user.clear(screen.getByRole("searchbox"));
    await user.type(screen.getByRole("searchbox"), "  crm  ");
    await waitFor(() => expect(marketParams(calls).filter((params) => params.get("q") === "crm")).toHaveLength(1));
    const sentQueries = marketParams(calls).flatMap((params) => {
      const q = params.get("q");
      return q === null ? [] : [q];
    });
    expect(sentQueries.length).toBeGreaterThan(0);
    for (const q of sentQueries) expect(q.trim()).toBe(q);
  });

  it("pages through windows and resets on a new query (CLOUD-023)", async () => {
    const { calls, transport } = catalogTransport({
      marketplace: (params) => {
        if ((params.get("q") ?? "") !== "") {
          return { status: 200, body: pageJson([appJson("app-query", "Query Hit")]) };
        }
        if (params.get("cursor") === "cursor-2") {
          return { status: 200, body: pageJson([appJson("app-c", "Window Two")]) };
        }
        return {
          status: 200,
          body: pageJson([appJson("app-a", "Window One A"), appJson("app-b", "Window One B")], "cursor-2"),
        };
      },
    });
    renderCatalog(transport);
    expect(await screen.findByText("Window One B")).toBeInTheDocument();
    expect(screen.getByTestId("apps-page-indicator")).toHaveTextContent("Page 1");
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Window Two")).toBeInTheDocument();
    expect(screen.getByTestId("apps-page-indicator")).toHaveTextContent("Page 2");
    const pageTwo = marketParams(calls).at(-1);
    expect(pageTwo?.get("cursor")).toBe("cursor-2");
    expect(pageTwo?.get("limit")).toBe("24");
    expect(screen.getByRole("button", { name: "Previous" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(await screen.findByText("Window One A")).toBeInTheDocument();
    expect(screen.getByTestId("apps-page-indicator")).toHaveTextContent("Page 1");
    // The first window reads cursorless; a fresh cache answers the revisit.
    expect(screen.getByText("Window One B")).toBeInTheDocument();

    // A new query mid-window drops the held cursor and the pager.
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Window Two")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.type(screen.getByRole("searchbox"), "query");
    expect(await screen.findByText("Query Hit")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Catalog pages" })).not.toBeInTheDocument();
    const filtered = marketParams(calls).filter((params) => params.get("q") === "query");
    expect(filtered).toHaveLength(1);
    expect(filtered[0]?.get("cursor")).toBeNull();
  });

  it("drives the catalog and its pager from the keyboard alone (CLOUD-039)", async () => {
    const user = userEvent.setup();
    const { transport } = catalogTransport({
      marketplace: (params) =>
        params.get("cursor") === "cursor-2"
          ? { status: 200, body: pageJson([appJson("app-c", "Window Two")]) }
          : {
              status: 200,
              body: pageJson([appJson("app-a", "Window One")], "cursor-2"),
            },
    });
    renderCatalog(transport);
    expect(await screen.findByText("Window One")).toBeInTheDocument();
    // The pager is a pair of ordinary buttons: focus and Enter page it.
    const next = screen.getByRole("button", { name: "Next" });
    next.focus();
    expect(next).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(await screen.findByText("Window Two")).toBeInTheDocument();
    expect(screen.getByTestId("apps-page-indicator")).toHaveTextContent("Page 2");
    const previous = screen.getByRole("button", { name: "Previous" });
    previous.focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByText("Window One")).toBeInTheDocument();
    expect(screen.getByTestId("apps-page-indicator")).toHaveTextContent("Page 1");
  });

  it("holds stale content and disables the pager while a window loads (CLOUD-023)", async () => {
    let resolveWindowTwo!: (answer: { status: number; body: unknown }) => void;
    const { transport } = catalogTransport({
      marketplace: (params) => {
        if (params.get("cursor") === "cursor-2") {
          return new Promise((resolve) => {
            resolveWindowTwo = resolve;
          });
        }
        return {
          status: 200,
          body: pageJson([appJson("app-a", "Window One A")], "cursor-2"),
        };
      },
    });
    renderCatalog(transport);
    expect(await screen.findByText("Window One A")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    // Stale window on screen, pager inert while it loads.
    await waitFor(() => expect(screen.getByRole("button", { name: "Next" })).toBeDisabled());
    expect(screen.getByText("Window One A")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();
    expect(screen.getByTestId("apps-catalog-grid")).toHaveAttribute("aria-busy", "true");

    resolveWindowTwo({ status: 200, body: pageJson([appJson("app-c", "Window Two")]) });
    expect(await screen.findByText("Window Two")).toBeInTheDocument();
  });

  it("retries a failed catalog read inline and never as empty (CLOUD-030)", async () => {
    const { calls, transport } = catalogTransport({
      marketplace: (_params, call) =>
        call === 1
          ? { status: 503, body: { error: "openhub_unavailable", detail: "Catalog is unreachable." } }
          : { status: 200, body: pageJson([appJson("app-back", "Back Online")]) },
    });
    renderCatalog(transport);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Catalog is unreachable.");
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Back Online")).toBeInTheDocument();
    expect(marketParams(calls)).toHaveLength(2);
  });

  it("distinguishes no apps from no matches (CLOUD-030)", async () => {
    const { transport } = catalogTransport({
      marketplace: () => ({ status: 200, body: pageJson([]) }),
    });
    renderCatalog(transport);

    expect(await screen.findByText(APPS_EMPTY_TITLE)).toBeInTheDocument();
    expect(screen.getByText(APPS_EMPTY_DESCRIPTION)).toBeInTheDocument();
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();

    const user = userEvent.setup();
    await user.type(screen.getByRole("searchbox"), "zzz-no-such-app");
    expect(await screen.findByText(APPS_NO_MATCHES_TITLE)).toBeInTheDocument();
    expect(screen.getByText(/zzz-no-such-app/)).toBeInTheDocument();
    expect(screen.queryByText(APPS_EMPTY_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("persists the agent-tools flip live (CLOUD-029)", async () => {
    const saved = { settings: { openhub: { apps_enabled: true } } } as unknown as Profile;
    updateProfileMock.mockResolvedValueOnce(saved);
    const { transport } = catalogTransport({
      marketplace: () => ({ status: 200, body: pageJson([]) }),
    });
    renderCatalog(transport);
    expect(await screen.findByText(APPS_EMPTY_TITLE)).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole("switch", { name: AGENT_TOOLS_LABEL }));
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: AGENT_TOOLS_LABEL })).toHaveAttribute("aria-checked", "true")
    );
    expect(updateProfileMock).toHaveBeenCalledWith(expect.anything(), {
      settings: { openhub: { apps_enabled: true } },
    });
  });

  it("hides the switch until the profile loads", async () => {
    const { transport } = catalogTransport({
      marketplace: () => ({ status: 200, body: pageJson([appJson("app-notes", "Notes Pro")]) }),
      profile: "hang",
    });
    renderCatalog(transport);
    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: AGENT_TOOLS_LABEL })).not.toBeInTheDocument();
  });

  it("shows a loading state for the first window", async () => {
    let resolveFirst!: (answer: { status: number; body: unknown }) => void;
    const { transport } = catalogTransport({
      marketplace: () =>
        new Promise((resolve) => {
          resolveFirst = resolve;
        }),
    });
    renderCatalog(transport);
    expect(await screen.findByText("Loading apps")).toBeInTheDocument();
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();

    resolveFirst({ status: 200, body: pageJson([appJson("app-notes", "Notes Pro")]) });
    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
  });

  it("installs from the card, refreshes, and toasts success (CLOUD-024)", async () => {
    let installed = false;
    const { calls, transport } = catalogTransport({
      marketplace: () => ({
        status: 200,
        body: pageJson([
          {
            ...appJson("app-plain", "Plain App"),
            authType: "none",
            isInstalled: installed,
            suggestedAction: installed ? "none" : "install",
          },
        ]),
      }),
      install: () => {
        installed = true;
        return { status: 200, body: { installed: true } };
      },
    });
    renderCatalog(transport);
    expect(await screen.findByText("Plain App")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Installed Plain App"));
    // The refresh rendered: the card now reads installed.
    expect(await screen.findByText("Installed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Uninstall" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install" })).not.toBeInTheDocument();
    expect(marketParams(calls)).toHaveLength(2);
  });

  it("connects a key app through the dialog and closes on success (CLOUD-027)", async () => {
    const { calls, transport } = catalogTransport({
      marketplace: () => ({
        status: 200,
        body: pageJson([
          {
            ...appJson("app-manual", "Manual CRM"),
            authType: "api_key",
            isInstalled: true,
            suggestedAction: "connect_auth",
          },
        ]),
      }),
      authStatus: () => ({
        status: 200,
        body: {
          type: "api_key",
          status: "needs_auth",
          manualSetup: {
            fields: [{ key: "api_key", label: "API key", type: "secret", placeholder: "sk-..." }],
          },
        },
      }),
    });
    renderCatalog(transport);
    expect(await screen.findByText("Manual CRM")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    const dialog = await screen.findByRole("dialog", { name: "Connect Manual CRM" });
    fireEvent.change(within(dialog).getByLabelText("API key"), { target: { value: "sk-abcd99" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Connect" }));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Connected Manual CRM"));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Connect Manual CRM" })).not.toBeInTheDocument());
    expect(calls.some((call) => call.url.includes("/auth/api-key/"))).toBe(true);
    await waitFor(() => expect(marketParams(calls).length).toBeGreaterThan(1));
  });

  it("removes from the card and reports (CLOUD-024)", async () => {
    let installed = true;
    const { transport } = catalogTransport({
      marketplace: () => ({
        status: 200,
        body: pageJson([
          {
            ...appJson("app-old", "Old App"),
            isInstalled: installed,
            suggestedAction: installed ? "none" : "install",
          },
        ]),
      }),
      uninstall: () => {
        installed = false;
        return { status: 200, body: { installed: false } };
      },
    });
    renderCatalog(transport);
    expect(await screen.findByText("Old App")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(notifyInfo).toHaveBeenCalledWith("Uninstalled Old App"));
    expect(await screen.findByRole("button", { name: "Install" })).toBeInTheDocument();
  });
});
