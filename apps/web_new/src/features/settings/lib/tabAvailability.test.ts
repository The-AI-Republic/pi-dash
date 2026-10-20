// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import { createClient } from "@pidash/api-client";
import { isTabAddressServed, loadSelfManagedMarker, tabHasInstanceGate } from "./tabAvailability.js";

function clientReturning(config: unknown): { queryClient: QueryClient; apiClient: ReturnType<typeof createClient> } {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const apiClient = createClient({
    baseUrl: "https://api.test",
    transport: async (url) => ({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ config }),
    }),
  });
  return { queryClient, apiClient };
}

const CONFIG = {
  enable_signup: true,
  is_workspace_creation_disabled: false,
  is_google_enabled: false,
  is_github_enabled: false,
  is_gitlab_enabled: false,
  is_gitea_enabled: false,
  is_magic_login_enabled: false,
  is_email_password_enabled: true,
  is_smtp_configured: false,
  is_self_managed: false,
  instance_changelog_url: "",
};

describe("profile tab address availability", () => {
  it("reads the instance marker", async () => {
    const { queryClient, apiClient } = clientReturning(CONFIG);
    await expect(loadSelfManagedMarker(queryClient, apiClient)).resolves.toBe(false);
  });

  it("answers unknown when the instance read fails", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const apiClient = createClient({
      baseUrl: "https://api.test",
      transport: async () => {
        throw new Error("offline");
      },
    });
    await expect(loadSelfManagedMarker(queryClient, apiClient)).resolves.toBeUndefined();
  });

  it("withholds the password address on a hosted instance", async () => {
    const { queryClient, apiClient } = clientReturning(CONFIG);
    await expect(isTabAddressServed({ queryClient, apiClient, tab: "security" })).resolves.toBe(false);
  });

  it("serves the password address on a self-managed instance", async () => {
    const { queryClient, apiClient } = clientReturning({ ...CONFIG, is_self_managed: true });
    await expect(isTabAddressServed({ queryClient, apiClient, tab: "security" })).resolves.toBe(true);
  });

  it("knows which tabs an instance can withhold", () => {
    expect(tabHasInstanceGate("security")).toBe(true);
    expect(tabHasInstanceGate("general")).toBe(false);
    expect(tabHasInstanceGate("api-tokens")).toBe(false);
  });

  it("serves an unrelated tab without consulting the marker", async () => {
    const { queryClient, apiClient } = clientReturning(CONFIG);
    let reads = 0;
    const watched = new Proxy(apiClient, {
      get(target, property, receiver) {
        if (property === "getJson") reads += 1;
        return Reflect.get(target, property, receiver);
      },
    });
    await expect(isTabAddressServed({ queryClient, apiClient: watched, tab: "general" })).resolves.toBe(true);
    await expect(isTabAddressServed({ queryClient, apiClient: watched, tab: "api-tokens" })).resolves.toBe(true);
    expect(reads).toBe(0);
  });
});
