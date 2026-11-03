// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { OpenhubApp, Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifyInfo: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifyInfo, notifySuccess } from "../../shared/shell/index.js";
import { openhubKeys } from "./api/keys.js";
import { useMarketplacePage } from "./api/queries.js";
import {
  COULD_NOT_CONNECT_TITLE,
  HUB_REFUSED_DESCRIPTION,
  INSTALL_FAILED_TITLE,
  INSTALLED_UNCONNECTED_TITLE,
  POPUP_BLOCKED_DESCRIPTION,
  UNINSTALL_FAILED_TITLE,
  connectedTitle,
  installedTitle,
  uninstalledTitle,
  useAppCardActions,
} from "./api/actions.js";

interface FlowCall {
  method: string;
  path: string;
}

interface FlowHandlers {
  install?: (appId: string) => { status: number; body: unknown };
  uninstall?: (appId: string) => { status: number; body: unknown };
  authStatus?: (appId: string) => { status: number; body: unknown };
  apiKey?: (appId: string) => { status: number; body: unknown };
}

function flowTransport(handlers: FlowHandlers = {}): { calls: FlowCall[]; transport: Transport } {
  const calls: FlowCall[] = [];
  const transport: Transport = async (url, init) => {
    const parsed = new URL(url);
    calls.push({ method: init.method, path: `${parsed.pathname}${parsed.search}` });
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
      return answer(200, { items: [], nextCursor: null });
    }
    const install = /^\/api\/openhub\/apps\/([^/]+)\/install\/$/.exec(parsed.pathname);
    if (install !== null) {
      const appId = install[1] ?? "";
      if (init.method === "POST") {
        const hit = handlers.install?.(appId) ?? { status: 200, body: { installed: true } };
        return answer(hit.status, hit.body);
      }
      const hit = handlers.uninstall?.(appId) ?? { status: 200, body: { installed: false } };
      return answer(hit.status, hit.body);
    }
    const auth = /^\/api\/openhub\/apps\/([^/]+)\/auth\/status\/$/.exec(parsed.pathname);
    if (auth !== null) {
      const hit = handlers.authStatus?.(auth[1] ?? "") ?? { status: 200, body: { type: "none", status: "active" } };
      return answer(hit.status, hit.body);
    }
    const key = /^\/api\/openhub\/apps\/([^/]+)\/auth\/api-key\/$/.exec(parsed.pathname);
    if (key !== null) {
      const hit = handlers.apiKey?.(key[1] ?? "") ?? { status: 200, body: { connected: true } };
      return answer(hit.status, hit.body);
    }
    throw new Error(`unexpected request ${init.method} ${url}`);
  };
  return { calls, transport };
}

/** Calls against the API itself (the client's CSRF prefetch is not under test). */
function apiCalls(calls: FlowCall[]): FlowCall[] {
  return calls.filter((call) => call.path.startsWith("/api/"));
}

function app(overrides: Partial<OpenhubApp> = {}): OpenhubApp {
  return { appId: "app-plain", slug: "plain", name: "Plain App", ...overrides };
}

interface FakeTab {
  closed: boolean;
  navigations: string[];
  close: () => void;
  window: Window;
}

function fakeTab(): FakeTab {
  const cheat: { closed: boolean; navigations: string[]; closeCalls: number } = {
    closed: false,
    navigations: [],
    closeCalls: 0,
  };
  const tab = {
    get closed() {
      return cheat.closed;
    },
    document: { title: "" },
    location: {},
    close: () => {
      cheat.closed = true;
    },
  } as unknown as Window & { location: { href?: string } };
  Object.defineProperty(tab.location, "href", {
    set(url: string) {
      cheat.navigations.push(url);
    },
    get() {
      return cheat.navigations.at(-1) ?? "about:blank";
    },
  });
  return {
    get closed() {
      return cheat.closed;
    },
    get navigations() {
      return cheat.navigations;
    },
    close: () => tab.close(),
    window: tab as unknown as Window,
  };
}

function seedStatus(queryClient: QueryClient, hubUrl: string | null = "https://hub.airepublic.com"): void {
  queryClient.setQueryData(openhubKeys.status("acme"), {
    connected: true,
    has_gateway_scopes: true,
    hub_url: hubUrl ?? undefined,
  });
}

function mountActions(queryClient: QueryClient) {
  const wrapper = function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
  const catalog = renderHook(() => useMarketplacePage("acme", { query: "", cursor: null }), { wrapper });
  const actions = renderHook(() => useAppCardActions("acme"), { wrapper });
  return { catalog, actions };
}

const realOpen = window.open;

beforeEach(() => {
  vi.clearAllMocks();
  window.history.replaceState(null, "", "/acme/apps/");
});

afterEach(() => {
  window.open = realOpen;
  resetAppClient();
});

function setup(handlers: FlowHandlers = {}, hubUrl: string | null = "https://hub.airepublic.com") {
  const { calls, transport } = flowTransport(handlers);
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  seedStatus(queryClient, hubUrl);
  return { calls, queryClient, ...mountActions(queryClient) };
}

describe("install", () => {
  it("commits, refreshes, then connects a plain app with a success notice (CLOUD-024)", async () => {
    const { actions, calls, catalog } = setup();
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.install(app({ authType: "none" }));
    });

    expect(notifySuccess).toHaveBeenCalledWith(installedTitle("Plain App"));
    expect(installedTitle("Plain App")).toContain("Installed");
    const paths = apiCalls(calls).map((call) => `${call.method} ${call.path.split("?")[0]}`);
    expect(paths).toEqual([
      "POST /api/openhub/apps/app-plain/install/",
      "GET /api/openhub/apps/marketplace/",
      "GET /api/openhub/apps/app-plain/auth/status/",
    ]);
    expect(actions.result.current.dialog).toBeNull();
  });

  it("reports a refused install and never starts connecting (CLOUD-024)", async () => {
    const tabs: Window[] = [];
    window.open = (() => {
      const tab = fakeTab();
      tabs.push(tab.window);
      return tab.window;
    }) as unknown as typeof window.open;
    const { actions, calls, catalog } = setup({
      install: () => ({ status: 502, body: { error: "openhub_error", detail: "Hub refused the install." } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.install(app({ authType: "oauth2" }));
    });

    expect(notifyError).toHaveBeenCalledWith(INSTALL_FAILED_TITLE, "Hub refused the install.");
    expect(apiCalls(calls)).toHaveLength(1);
    expect(tabs).toHaveLength(1);
    expect(tabs[0]?.closed).toBe(true);
    expect(actions.result.current.dialog).toBeNull();
  });

  it("opens the credential dialog for key apps without a toast (CLOUD-024/027)", async () => {
    const open = vi.fn(() => null);
    window.open = open as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "api_key", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ authType: "api_key", name: "Manual CRM" }));
    });

    expect(open).not.toHaveBeenCalled();
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
    expect(actions.result.current.dialog?.app.name).toBe("Manual CRM");
    expect(actions.result.current.dialog?.auth.type).toBe("api_key");
  });

  it("succeeds quietly when a key app is already connected (CLOUD-024)", async () => {
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "api_key", status: "connected" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ authType: "api_key" }));
    });

    expect(notifySuccess).toHaveBeenCalledWith(installedTitle("Plain App"));
    expect(actions.result.current.dialog).toBeNull();
  });

  it("points the reserved tab at the consent handoff for provider apps (CLOUD-024/025)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    window.history.replaceState(null, "", "/acme/apps/?error=stale&connected=1&kept=yes");
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ appId: "app-oauth-x", authType: "oauth2" }));
    });

    expect(window.open).toHaveBeenCalledTimes(1);
    expect(tab.navigations).toEqual([
      "https://api.test/api/openhub/apps/app-oauth-x/auth/oauth/begin/?return_path=%2Facme%2Fapps%2F%3Fkept%3Dyes",
    ]);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("treats an omitted mechanism as provider (CLOUD-025)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ authType: undefined }));
    });

    expect(window.open).toHaveBeenCalledTimes(1);
    expect(tab.navigations).toHaveLength(1);
  });

  it("explains a blocked popup with a partial notice (CLOUD-024/025)", async () => {
    window.open = vi.fn(() => null) as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ authType: "oauth2" }));
    });

    expect(notifyError).toHaveBeenCalledWith(INSTALLED_UNCONNECTED_TITLE, POPUP_BLOCKED_DESCRIPTION);
    expect(POPUP_BLOCKED_DESCRIPTION.toLowerCase()).toContain("pop-up");
  });

  it("opens the hub detail page for connected provider apps (CLOUD-031)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(
        app({ appId: "com.linear.linear", slug: "linear", name: "Linear", authType: "oauth2" })
      );
    });

    expect(tab.navigations).toEqual(["https://hub.airepublic.com/apps/linear"]);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("refuses unsafe hub bases, closes the tab, and reports partially (CLOUD-024/031)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup(
      {
        authStatus: () => ({ status: 200, body: { type: "oauth2", status: "connected" } }),
      },
      "https://user:pass@hub.example.com"
    );
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ authType: "oauth2" }));
    });

    expect(tab.closed).toBe(true);
    expect(tab.navigations).toHaveLength(0);
    expect(notifyError).toHaveBeenCalledWith(INSTALLED_UNCONNECTED_TITLE, HUB_REFUSED_DESCRIPTION);
  });

  it("reports partially when the status answer is missing its hub base (CLOUD-024/031)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup(
      {
        authStatus: () => ({ status: 200, body: { type: "oauth2", status: "connected" } }),
      },
      null
    );
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.install(app({ authType: "oauth2" }));
    });

    expect(tab.closed).toBe(true);
    expect(notifyError).toHaveBeenCalledWith(INSTALLED_UNCONNECTED_TITLE, HUB_REFUSED_DESCRIPTION);
  });

  it("reports partially when no status answer is cached (CLOUD-024/031)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog, queryClient } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    queryClient.removeQueries({ queryKey: openhubKeys.status("acme") });

    await act(async () => {
      await actions.result.current.install(app({ authType: "oauth2" }));
    });

    expect(tab.closed).toBe(true);
    expect(notifyError).toHaveBeenCalledWith(INSTALLED_UNCONNECTED_TITLE, HUB_REFUSED_DESCRIPTION);
  });

  it("reports partially and closes the tab when the connect leg fails (CLOUD-024)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, calls, catalog } = setup({
      authStatus: () => ({ status: 503, body: { error: "openhub_error", detail: "Status blew up." } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.install(app({ authType: "oauth2" }));
    });

    expect(tab.closed).toBe(true);
    expect(notifyError).toHaveBeenCalledWith(INSTALLED_UNCONNECTED_TITLE, "Status blew up.");
    // The commit still refreshed before the connect threw.
    const methods = apiCalls(calls).map((call) => call.method);
    expect(methods).toEqual(["POST", "GET", "GET"]);
  });
});

describe("remove", () => {
  it("deletes, refreshes, then reports (CLOUD-024)", async () => {
    const { actions, calls, catalog } = setup();
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.remove(app({ name: "Old App" }));
    });

    const paths = apiCalls(calls).map((call) => `${call.method} ${call.path.split("?")[0]}`);
    expect(paths).toEqual(["DELETE /api/openhub/apps/app-plain/install/", "GET /api/openhub/apps/marketplace/"]);
    expect(notifyInfo).toHaveBeenCalledWith(uninstalledTitle("Old App"));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports a rejected removal (CLOUD-024)", async () => {
    const { actions, calls, catalog } = setup({
      uninstall: () => ({ status: 502, body: { error: "openhub_error", detail: "Hub refused the removal." } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.remove(app());
    });

    expect(notifyError).toHaveBeenCalledWith(UNINSTALL_FAILED_TITLE, "Hub refused the removal.");
    expect(notifyInfo).not.toHaveBeenCalled();
    expect(apiCalls(calls)).toHaveLength(1);
  });
});

describe("connect", () => {
  it("opens the dialog for key apps that need one (CLOUD-027)", async () => {
    const open = vi.fn(() => null);
    window.open = open as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "api_key", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.connect(app({ authType: "api_key" }));
    });

    expect(open).not.toHaveBeenCalled();
    expect(actions.result.current.dialog?.auth.type).toBe("api_key");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("confirms and refreshes when a key app is already connected", async () => {
    const { actions, calls, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "api_key", status: "connected" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.connect(app({ authType: "api_key" }));
    });

    expect(notifySuccess).toHaveBeenCalledWith(connectedTitle("Plain App"));
    expect(connectedTitle("Plain App")).toContain("Connected");
    expect(apiCalls(calls).map((call) => call.path.split("?")[0])).toEqual([
      "/api/openhub/apps/app-plain/auth/status/",
      "/api/openhub/apps/marketplace/",
    ]);
    expect(actions.result.current.dialog).toBeNull();
  });

  it("treats none-typed apps as already usable", async () => {
    const { actions, catalog } = setup();
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.connect(app({ authType: "none" }));
    });

    expect(notifySuccess).toHaveBeenCalledWith(connectedTitle("Plain App"));
    expect(actions.result.current.dialog).toBeNull();
  });

  it("points the reserved tab at the handoff for provider apps (CLOUD-025)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.connect(app({ authType: "oauth2" }));
    });

    expect(tab.navigations).toEqual([
      "https://api.test/api/openhub/apps/app-plain/auth/oauth/begin/?return_path=%2Facme%2Fapps%2F",
    ]);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("opens the hub detail page for connected provider apps (CLOUD-031)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "connected" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.connect(app({ slug: "linear", authType: "oauth2" }));
    });

    expect(tab.navigations).toEqual(["https://hub.airepublic.com/apps/linear"]);
  });

  it("explains a blocked popup without installing anything (CLOUD-025)", async () => {
    window.open = vi.fn(() => null) as unknown as typeof window.open;
    const { actions, calls, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "oauth2", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    await act(async () => {
      await actions.result.current.connect(app({ authType: "oauth2" }));
    });

    expect(notifyError).toHaveBeenCalledWith(COULD_NOT_CONNECT_TITLE, POPUP_BLOCKED_DESCRIPTION);
    expect(calls.some((call) => call.path.endsWith("/install/"))).toBe(false);
  });

  it("reports when the connect read fails (CLOUD-027)", async () => {
    const tab = fakeTab();
    window.open = vi.fn(() => tab.window) as unknown as typeof window.open;
    const { actions, catalog } = setup({
      authStatus: () => ({ status: 503, body: { error: "openhub_error", detail: "Status blew up." } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    await act(async () => {
      await actions.result.current.connect(app({ authType: "oauth2" }));
    });

    expect(tab.closed).toBe(true);
    expect(notifyError).toHaveBeenCalledWith(COULD_NOT_CONNECT_TITLE, "Status blew up.");
    expect(actions.result.current.dialog).toBeNull();
  });
});

describe("submitCredentials", () => {
  it("confirms an accepted credential (CLOUD-027)", async () => {
    const { actions, calls, catalog } = setup();
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    calls.length = 0;

    let ok = false;
    await act(async () => {
      ok = await actions.result.current.submitCredentials(app(), { api_key: "sk-abcd99" });
    });

    expect(ok).toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith(connectedTitle("Plain App"));
    expect(apiCalls(calls).map((call) => `${call.method} ${call.path.split("?")[0]}`)).toEqual([
      "POST /api/openhub/apps/app-plain/auth/api-key/",
    ]);
  });

  it("stays failed with the rejection when the service refuses (CLOUD-027)", async () => {
    const { actions, catalog } = setup({
      apiKey: () => ({
        status: 400,
        body: { error: "openhub_invalid_argument", detail: "That credential was rejected." },
      }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());

    let ok = true;
    await act(async () => {
      ok = await actions.result.current.submitCredentials(app(), { api_key: "sk-wrong1" });
    });

    expect(ok).toBe(false);
    expect(notifyError).toHaveBeenCalledWith(COULD_NOT_CONNECT_TITLE, "That credential was rejected.");
  });
});

describe("closeDialog", () => {
  it("drops the dialog and refreshes the list (CLOUD-027)", async () => {
    const { actions, calls, catalog } = setup({
      authStatus: () => ({ status: 200, body: { type: "api_key", status: "needs_auth" } }),
    });
    await waitFor(() => expect(catalog.result.current.data).toBeDefined());
    await act(async () => {
      await actions.result.current.connect(app({ authType: "api_key" }));
    });
    expect(actions.result.current.dialog).not.toBeNull();
    calls.length = 0;

    act(() => {
      actions.result.current.closeDialog();
    });

    expect(actions.result.current.dialog).toBeNull();
    await waitFor(() => {
      expect(calls.some((call) => call.path.startsWith("/api/openhub/apps/marketplace/"))).toBe(true);
    });
  });
});
