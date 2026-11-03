// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SignUpCard } from "../index.js";

const platformHarness = vi.hoisted(() => ({ shell: undefined as unknown }));

vi.mock("../../../core/edition/index.js", () => ({
  edition: {
    id: "cloud",
    flags: {},
    auth: [{ id: "airepublic", label: "AI Republic" }],
  },
}));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return { kind: "web", shellSignIn: platformHarness.shell };
  },
  toApiTransport: () => {
    throw new Error("unexpected transport resolution");
  },
}));

interface StubRoute {
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: 200,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function configBody(): string {
  return JSON.stringify({
    config: {
      enable_signup: true,
      is_workspace_creation_disabled: false,
      is_google_enabled: false,
      is_github_enabled: false,
      is_gitlab_enabled: false,
      is_gitea_enabled: false,
      is_magic_login_enabled: true,
      is_email_password_enabled: true,
      is_smtp_configured: true,
      is_self_managed: false,
      instance_changelog_url: "",
    },
  });
}

async function renderReadyCard(): Promise<void> {
  createAppClient(
    "https://api.test",
    stubTransport((url) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { body: CSRF, url };
      if (url.endsWith("/api/instances/")) return { body: configBody(), url };
      throw new Error(`unexpected ${url}`);
    })
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SignUpCard />
    </QueryClientProvider>
  );
  await screen.findByRole("button", { name: /ai republic/i });
}

describe("generic card provider button", () => {
  const assign = vi.fn();

  beforeEach(() => {
    assign.mockClear();
    platformHarness.shell = undefined;
    Object.defineProperty(globalThis.window, "location", {
      value: { origin: "https://app.test", pathname: "/sign-up", search: "?next_path=%2Fdocs", hash: "#frag", assign },
      writable: true,
      configurable: true,
    });
  });

  afterEach(() => {
    resetAppClient();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("offers the provider button alongside the standard continue", async () => {
    await renderReadyCard();
    expect(screen.getByRole("button", { name: /ai republic/i })).toBeVisible();
    expect(screen.getByRole("button", { name: "Continue" })).toBeVisible();
  });

  it("carries the page without the fragment through the handoff", async () => {
    const user = userEvent.setup();
    await renderReadyCard();
    await user.click(screen.getByRole("button", { name: /ai republic/i }));
    expect(assign).toHaveBeenCalledTimes(1);
    const url = new URL(String(assign.mock.calls[0]?.[0]));
    expect(`${url.origin}${url.pathname}`).toBe("https://api.test/api/auth/sign-in/");
    expect(url.searchParams.get("return_to")).toBe("/sign-up?next_path=%2Fdocs");
  });

  it("opens the system browser inside the shell instead of navigating", async () => {
    const openSystemBrowser = vi.fn(async (_url: string) => undefined);
    platformHarness.shell = { bakedOrigin: "https://api.test", openSystemBrowser };
    const user = userEvent.setup();
    await renderReadyCard();
    await user.click(screen.getByRole("button", { name: /ai republic/i }));
    expect(assign).not.toHaveBeenCalled();
    expect(openSystemBrowser).toHaveBeenCalledTimes(1);
    const url = new URL(String(openSystemBrowser.mock.calls[0]?.[0]));
    expect(url.searchParams.get("desktop")).toBe("1");
    expect(url.searchParams.get("return_to")).toBe("/sign-up?next_path=%2Fdocs");
  });

  it("refuses loudly in the shell without a baked origin", async () => {
    platformHarness.shell = { bakedOrigin: undefined, openSystemBrowser: vi.fn() };
    const logged: string[] = [];
    vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
      logged.push(String(message));
    });
    const user = userEvent.setup();
    await renderReadyCard();
    await user.click(screen.getByRole("button", { name: /ai republic/i }));
    expect(assign).not.toHaveBeenCalled();
    expect(logged.some((line) => line.includes("VITE_API_BASE_URL"))).toBe(true);
  });
});
