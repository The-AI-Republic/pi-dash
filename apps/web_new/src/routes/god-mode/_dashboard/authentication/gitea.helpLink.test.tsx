// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// ADM-030, NEWFRONT-492: the Gitea route's real wiring (definition,
// credential set, copy help) over the provider page, focused on the
// callback help link. The link follows the host typed into the form on
// the same render and falls back to the public host the moment the
// field is cleared — it never shows a previous host.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { adminKeys } from "../../../../features/admin/queries.js";
import { resetAdminStores } from "../../../../features/admin/stores.js";
import { Route } from "./gitea.js";

const PUBLIC_APPLICATIONS = "https://gitea.com/user/settings/applications";

function configRow(key: string, value: string | null) {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "AUTHENTICATION",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

const CONFIG: Record<string, string> = {
  IS_GITEA_ENABLED: "0",
  GITEA_HOST: "",
  GITEA_CLIENT_ID: "",
  GITEA_CLIENT_SECRET: "",
  ENABLE_GITEA_SYNC: "0",
};

function seedConfig(queryClient: QueryClient): void {
  queryClient.setQueryData(
    adminKeys.configurations,
    Object.entries(CONFIG).map(([key, value]) => configRow(key, value))
  );
}

function renderGiteaRoute(): void {
  const transport = (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: () =>
      Promise.resolve(
        JSON.stringify(
          url.includes("get-csrf-token")
            ? { csrf_token: "csrf" }
            : Object.entries(CONFIG).map(([k, v]) => configRow(k, v))
        )
      ),
  })) as unknown as Transport;
  createAppClient("https://api.example.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  seedConfig(queryClient);
  const Component = Route.options.component;
  if (!Component) throw new Error("gitea authentication route has no component");
  render(
    <QueryClientProvider client={queryClient}>
      <Component />
    </QueryClientProvider>
  );
}

function helpHref(): string | null {
  return screen.getByRole("link", { name: "Open this host's application settings" }).getAttribute("href");
}

function changeHost(value: string): void {
  fireEvent.change(screen.getByTestId("admin-auth-field-GITEA_HOST"), { target: { value } });
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("Gitea route callback help link", () => {
  it("starts on the public host's applications and follows the host as it is typed", async () => {
    renderGiteaRoute();
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());
    expect(screen.getByTestId("admin-auth-field-GITEA_HOST")).toHaveValue("https://gitea.com");
    expect(helpHref()).toBe(PUBLIC_APPLICATIONS);

    changeHost("https://gitea.parity.invalid");
    expect(helpHref()).toBe("https://gitea.parity.invalid/user/settings/applications");

    changeHost("https://gitea.other.invalid/");
    expect(helpHref()).toBe("https://gitea.other.invalid/user/settings/applications");
  });

  it("falls back to the public host the moment the field clears, with no stale host", async () => {
    renderGiteaRoute();
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());

    changeHost("https://gitea.parity.invalid");
    expect(helpHref()).toBe("https://gitea.parity.invalid/user/settings/applications");

    changeHost("");
    expect(helpHref()).toBe(PUBLIC_APPLICATIONS);
  });
});
