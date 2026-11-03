// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider page: the shell over the shared configurations fetch with
// the persisting master toggle (no last-method guard), the in-flight
// and pre-load locks, and the form once the map arrives.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { adminKeys } from "../queries.js";
import { AUTH_PROVIDER_BY_SLUG } from "../providerAuth.js";
import { resetAdminStores } from "../stores.js";
import { AdminAuthProviderPage } from "./AdminAuthProviderPage.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

const mockedNotifySuccess = vi.mocked(notifySuccess);
const mockedNotifyError = vi.mocked(notifyError);

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
  IS_GITHUB_ENABLED: "0",
  GITHUB_CLIENT_ID: "github-id",
  GITHUB_CLIENT_SECRET: "github-secret",
  ENABLE_GITHUB_SYNC: "0",
};

function seedConfig(queryClient: QueryClient, values: Record<string, string>): void {
  queryClient.setQueryData(
    adminKeys.configurations,
    Object.entries(values).map(([key, value]) => configRow(key, value))
  );
}

interface PageStub {
  bodies: unknown[];
  release: () => void;
  transport: Transport;
}

function stubPageTransport(args: {
  getStatus?: number;
  hangGet?: boolean;
  patchStatus?: number;
  hangPatch?: boolean;
}): PageStub {
  const bodies: unknown[] = [];
  let release = () => {};
  const gate = args.hangPatch === true ? new Promise<void>((resolve) => (release = resolve)) : null;
  const transport = (async (url: string, init?: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf" })),
      };
    }
    const method = init?.method ?? "GET";
    const path = new URL(url, "https://api.test").pathname;
    if (path === "/api/instances/configurations/" && method === "GET") {
      if (args.hangGet === true) await new Promise(() => {});
      return {
        status: args.getStatus ?? 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(args.getStatus === 500 ? { detail: "boom" } : [])),
      };
    }
    if (path === "/api/instances/configurations/" && method === "PATCH") {
      bodies.push(init?.body === undefined ? null : JSON.parse(init.body));
      if (gate !== null) await gate;
      const answered = Object.entries((bodies[bodies.length - 1] ?? {}) as Record<string, string>).map(([key, value]) =>
        configRow(key, value)
      );
      return {
        status: args.patchStatus ?? 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(args.patchStatus === 500 ? { detail: "boom" } : answered)),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: () => Promise.resolve("{}") };
  }) as Transport;
  return { bodies, release, transport };
}

function renderPage(queryClient: QueryClient): void {
  render(
    <QueryClientProvider client={queryClient}>
      <AdminAuthProviderPage provider={AUTH_PROVIDER_BY_SLUG.github} />
    </QueryClientProvider>
  );
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("provider page", () => {
  it("shows the locked header over the skeleton before configuration loads", () => {
    createAppClient("https://api.test", stubPageTransport({ hangGet: true }).transport);
    renderPage(freshClient());

    expect(screen.getByTestId("admin-auth-provider-name")).toHaveTextContent("GitHub");
    expect(screen.getByTestId("admin-auth-provider-toggle")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("admin-auth-provider-skeleton").children).toHaveLength(5);
    expect(screen.queryByTestId("admin-auth-provider-form")).not.toBeInTheDocument();
    expect(document.title).toBe("GitHub Authentication - God Mode");
  });

  it("keeps the skeleton when the fetch fails", async () => {
    createAppClient("https://api.test", stubPageTransport({ getStatus: 500 }).transport);
    renderPage(freshClient());

    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-skeleton")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-auth-provider-form")).not.toBeInTheDocument();
  });

  it("mounts the form with the stored toggle once configuration loads", async () => {
    createAppClient("https://api.test", stubPageTransport({}).transport);
    const queryClient = freshClient();
    seedConfig(queryClient, CONFIG);
    renderPage(queryClient);

    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());
    expect(screen.getByTestId("admin-auth-provider-toggle")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("admin-auth-provider-toggle")).not.toHaveAttribute("aria-disabled", "true");
    expect(screen.queryByTestId("admin-auth-provider-skeleton")).not.toBeInTheDocument();
  });

  it("persists the master toggle on with the active notice", async () => {
    const stub = stubPageTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, CONFIG);
    renderPage(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-provider-toggle"));
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith("Configuration saved", "GitHub authentication is now active.")
    );
    expect(stub.bodies).toEqual([{ IS_GITHUB_ENABLED: "1" }]);
    expect(screen.getByTestId("admin-auth-provider-toggle")).toHaveAttribute("aria-checked", "true");
    expect(mockedNotifyError).not.toHaveBeenCalled();
  });

  it("persists the master toggle off with the disabled notice and no last-method guard", async () => {
    const stub = stubPageTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, { ...CONFIG, IS_GITHUB_ENABLED: "1" });
    renderPage(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-provider-toggle"));
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith("Configuration saved", "GitHub authentication is now disabled.")
    );
    expect(stub.bodies).toEqual([{ IS_GITHUB_ENABLED: "0" }]);
  });

  it("locks the master toggle while its save flies", async () => {
    const stub = stubPageTransport({ hangPatch: true });
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, CONFIG);
    renderPage(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-provider-toggle"));
    await waitFor(() =>
      expect(screen.getByTestId("admin-auth-provider-toggle")).toHaveAttribute("aria-disabled", "true")
    );
    stub.release();
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith("Configuration saved", "GitHub authentication is now active.")
    );
    expect(screen.getByTestId("admin-auth-provider-toggle")).not.toHaveAttribute("aria-disabled", "true");
  });

  it("reports a failed master save and keeps the stored toggle", async () => {
    const stub = stubPageTransport({ patchStatus: 500 });
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, CONFIG);
    const errored = vi.spyOn(console, "error").mockImplementation(() => {});
    renderPage(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-provider-toggle"));
    await waitFor(() => expect(mockedNotifyError).toHaveBeenCalledWith("Error", "Failed to save configuration"));
    expect(stub.bodies).toEqual([{ IS_GITHUB_ENABLED: "1" }]);
    expect(
      errored.mock.calls.some((call) => String(call[0]).includes("Error updating the instance configurations"))
    ).toBe(true);
    expect(screen.getByTestId("admin-auth-provider-toggle")).toHaveAttribute("aria-checked", "false");
    expect(mockedNotifySuccess).not.toHaveBeenCalled();
  });

  it("forwards credential field sets to the form", async () => {
    createAppClient("https://api.test", stubPageTransport({}).transport);
    const queryClient = freshClient();
    seedConfig(queryClient, { ...CONFIG, CLIENT_ID: "forwarded-id" });
    render(
      <QueryClientProvider client={queryClient}>
        <AdminAuthProviderPage
          provider={AUTH_PROVIDER_BY_SLUG.github}
          fields={[{ key: "CLIENT_ID", label: "Client ID", type: "text", required: true }]}
        />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("admin-auth-provider-form")).toBeInTheDocument());
    expect(screen.getByTestId("admin-auth-field-CLIENT_ID")).toHaveValue("forwarded-id");
  });
});
