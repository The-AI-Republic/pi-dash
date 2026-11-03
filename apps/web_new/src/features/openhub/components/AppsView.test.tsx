// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import type { OpenhubMarketplacePage, Profile, Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/queries.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { openhubKeys } from "../api/keys.js";
import {
  APPS_DISCONNECTED_FALLBACK,
  APPS_STALE_BODY,
  APPS_STALE_HEADING,
  APPS_UNAVAILABLE_HEADING,
  AppsView,
  CONNECTION_FAILED_MESSAGE,
  CONNECTION_FAILED_TITLE,
} from "./AppsView.js";

type StatusAnswer = { status: number; body: unknown } | Promise<{ status: number; body: unknown }>;

function statusTransport(handler: (call: number) => StatusAnswer): { calls: string[]; transport: Transport } {
  const calls: string[] = [];
  let seen = 0;
  const transport: Transport = async (url, init) => {
    calls.push(`${init.method} ${url}`);
    const parsed = new URL(url);
    if (parsed.pathname === "/api/openhub/status/") {
      seen += 1;
      const answer = await handler(seen);
      return {
        status: answer.status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(answer.body),
      };
    }
    throw new Error(`unexpected request ${init.method} ${url}`);
  };
  return { calls, transport };
}

function connectedStatus(): Record<string, unknown> {
  return { connected: true, has_gateway_scopes: true, hub_url: "https://hub.test" };
}

function renderView(transport: Transport, seed?: (queryClient: QueryClient) => void, staleTime = 30_000): QueryClient {
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime } },
  });
  seed?.(queryClient);
  render(
    <QueryClientProvider client={queryClient}>
      <AppsView workspaceSlug="acme" />
    </QueryClientProvider>
  );
  return queryClient;
}

function seedCatalog(queryClient: QueryClient): void {
  queryClient.setQueryData(openhubKeys.marketplacePage("acme", "", null), {
    items: [{ appId: "app-notes", slug: "notes", name: "Notes Pro" }],
    nextCursor: null,
  } as OpenhubMarketplacePage);
  queryClient.setQueryData(sessionKeys.profile, {
    settings: { openhub: { apps_enabled: false } },
  } as unknown as Profile);
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  window.history.replaceState(null, "", "/");
  resetAppClient();
});

describe("AppsView", () => {
  it("shows a loading state while the status read is in flight", async () => {
    let resolveStatus!: (answer: { status: number; body: unknown }) => void;
    const { transport } = statusTransport(
      () =>
        new Promise((resolve) => {
          resolveStatus = resolve;
        })
    );
    renderView(transport, seedCatalog);
    expect(await screen.findByText("Loading apps")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Apps", level: 1 })).toBeInTheDocument();

    resolveStatus({ status: 200, body: connectedStatus() });
    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
  });

  it("retries a failed status read and lands on the live state (CLOUD-030)", async () => {
    const { calls, transport } = statusTransport((call) =>
      call === 1
        ? { status: 500, body: { error: "openhub_error", detail: "Status check blew up." } }
        : { status: 200, body: { connected: false, has_gateway_scopes: false, hub_url: "https://hub.test" } }
    );
    renderView(transport, seedCatalog);

    expect(await screen.findByText("Status check blew up.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: APPS_UNAVAILABLE_HEADING })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByText(APPS_STALE_BODY)).toBeInTheDocument();
    expect(calls.filter((call) => call.includes("/api/openhub/status/"))).toHaveLength(2);
  });

  it("renders the error panel when the status read never answers (CLOUD-030, NEWFRONT-473)", async () => {
    const { transport } = statusTransport(() => {
      throw new Error("socket hang up");
    });
    renderView(transport, seedCatalog);

    expect(await screen.findByText(/could not be reached/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: APPS_UNAVAILABLE_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    // An error, never the empty catalog: no chrome, no grid.
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();
    expect(screen.queryByText("No apps available")).not.toBeInTheDocument();
  });

  it("points stale sessions at sign-out-and-back-in with no catalog chrome (CLOUD-030)", async () => {
    const { transport } = statusTransport(() => ({
      status: 200,
      body: { connected: false, has_gateway_scopes: false, hub_url: "https://hub.test" },
    }));
    renderView(transport, seedCatalog);

    expect(await screen.findByText(APPS_STALE_BODY)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: APPS_STALE_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Apps", level: 1 })).toBeInTheDocument();
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();
  });

  it("shows the server detail for scoped-but-disconnected sessions (CLOUD-030)", async () => {
    const { transport } = statusTransport(() => ({
      status: 200,
      body: {
        connected: false,
        has_gateway_scopes: true,
        hub_url: "https://hub.test",
        detail: "Hub is down for maintenance.",
      },
    }));
    renderView(transport, seedCatalog);

    expect(await screen.findByText("Hub is down for maintenance.")).toBeInTheDocument();
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();
  });

  it("falls back to an unavailable line without a server detail (CLOUD-030)", async () => {
    const { transport } = statusTransport(() => ({
      status: 200,
      body: { connected: false, has_gateway_scopes: true, hub_url: "https://hub.test" },
    }));
    renderView(transport, seedCatalog);

    expect(await screen.findByText(APPS_DISCONNECTED_FALLBACK)).toBeInTheDocument();
    expect(screen.queryByTestId("apps-catalog-grid")).not.toBeInTheDocument();
  });

  it("mounts the catalog behind a connected status (CLOUD-022)", async () => {
    const { transport } = statusTransport(() => ({ status: 200, body: connectedStatus() }));
    renderView(transport, seedCatalog);

    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
    expect(screen.getByRole("searchbox")).toBeInTheDocument();
    expect(screen.getByTestId("apps-catalog-grid")).toBeInTheDocument();
  });

  it("holds the loading state when the settled read carries no data", async () => {
    const { transport } = statusTransport(() => ({ status: 200, body: connectedStatus() }));
    renderView(
      transport,
      (queryClient) => {
        queryClient.setQueryData(openhubKeys.status("acme"), undefined);
      },
      Number.POSITIVE_INFINITY
    );

    expect(await screen.findByText("Loading apps")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reports a bounced error once and cleans the address on any gate state (CLOUD-026)", async () => {
    window.history.replaceState(null, "", "/acme/apps/?error=openhub_unavailable&connected=1&kept=yes");
    const { transport } = statusTransport(() => ({
      status: 200,
      body: { connected: false, has_gateway_scopes: true, hub_url: "https://hub.test" },
    }));
    renderView(transport, seedCatalog);

    // Above the gate: the disconnected page reports too.
    expect(await screen.findByText(APPS_DISCONNECTED_FALLBACK)).toBeInTheDocument();
    expect(notifyError).toHaveBeenCalledTimes(1);
    expect(notifyError).toHaveBeenCalledWith(CONNECTION_FAILED_TITLE, CONNECTION_FAILED_MESSAGE);
    expect(CONNECTION_FAILED_TITLE).toContain("Connection failed");
    // Stripped, with the unrelated param kept: later searches re-run
    // the effect against a clean address and stay silent.
    expect(window.location.pathname).toBe("/acme/apps/");
    expect(window.location.search).toBe("?kept=yes");
  });

  it("strips a lone outcome param without reporting (CLOUD-026)", async () => {
    window.history.replaceState(null, "", "/acme/apps/?connected=1");
    const { transport } = statusTransport(() => ({ status: 200, body: connectedStatus() }));
    renderView(transport, seedCatalog);

    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
    expect(notifyError).not.toHaveBeenCalled();
    expect(window.location.pathname).toBe("/acme/apps/");
    expect(window.location.search).toBe("");
  });

  it("leaves addresses without outcome params alone (CLOUD-026)", async () => {
    window.history.replaceState(null, "", "/acme/apps/?kept=yes");
    const { transport } = statusTransport(() => ({ status: 200, body: connectedStatus() }));
    renderView(transport, seedCatalog);

    expect(await screen.findByText("Notes Pro")).toBeInTheDocument();
    expect(notifyError).not.toHaveBeenCalled();
    expect(window.location.search).toBe("?kept=yes");
  });
});
