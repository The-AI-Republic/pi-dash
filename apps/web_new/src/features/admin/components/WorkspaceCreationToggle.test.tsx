// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { adminKeys } from "../queries.js";
import { WorkspaceCreationToggle } from "./WorkspaceCreationToggle.js";

function configRow(key: string, value: string | null): Record<string, unknown> {
  return {
    id: "77777777-7777-4777-8777-777777777777",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "test",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

function jsonResponse(url: string, status: number, body: unknown) {
  return Promise.resolve({
    status,
    url,
    headers: { get: () => null },
    text: () => Promise.resolve(JSON.stringify(body)),
  });
}

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

function renderToggle(queryClient?: QueryClient): QueryClient {
  const client = queryClient ?? new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <WorkspaceCreationToggle />
    </QueryClientProvider>
  );
  return client;
}

function restrictSwitch(): HTMLElement {
  return screen.getByRole("switch", { name: "Restrict workspace creation" });
}

afterEach(() => {
  resetAppClient();
});

describe("WorkspaceCreationToggle", () => {
  it("shows a skeleton row while configuration loads", () => {
    createAppClient("https://api.test", hangingTransport());
    renderToggle();
    expect(screen.getByTestId("admin-workspace-toggle-skeleton")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("reads the stored flag once configuration lands", async () => {
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "1")])) as Transport);
    renderToggle();
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "true"));
    expect(screen.queryByTestId("admin-workspace-toggle-skeleton")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Restrict workspace creation" })).toBeInTheDocument();
  });

  it("reads a missing flag as open", async () => {
    createAppClient("https://api.test", ((url: string) =>
      jsonResponse(url, 200, [configRow("OTHER", "1")])) as Transport);
    renderToggle();
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "false"));
  });

  it("persists a flip as a single-key write and re-reads the stored value", async () => {
    const bodies: Array<string | undefined> = [];
    const transport: Transport = (url, init) => {
      const path = new URL(url, "https://api.test").pathname;
      if (init.method === "PATCH" && path === "/api/instances/configurations/") {
        bodies.push(init.body);
        return jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "1")]);
      }
      return jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "0")]);
    };
    createAppClient("https://api.test", transport);
    renderToggle();
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "false"));
    fireEvent.click(restrictSwitch());
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "true"));
    expect(bodies).toEqual([JSON.stringify({ DISABLE_WORKSPACE_CREATION: "1" })]);
  });

  it("locks the switch while a save is in flight", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const transport: Transport = (url, init) => {
      const path = new URL(url, "https://api.test").pathname;
      if (init.method === "PATCH" && path === "/api/instances/configurations/") {
        return gate.then(() => jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "1")]));
      }
      return jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "0")]);
    };
    createAppClient("https://api.test", transport);
    renderToggle();
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "false"));
    fireEvent.click(restrictSwitch());
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("data-disabled"));
    release();
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "true"));
    expect(restrictSwitch()).not.toHaveAttribute("data-disabled");
  });

  it("keeps the stored value when a save fails", async () => {
    const transport: Transport = (url, init) => {
      const path = new URL(url, "https://api.test").pathname;
      if (init.method === "PATCH" && path === "/api/instances/configurations/") {
        return jsonResponse(url, 500, { detail: "boom" });
      }
      return jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "0")]);
    };
    createAppClient("https://api.test", transport);
    renderToggle();
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "false"));
    fireEvent.click(restrictSwitch());
    await waitFor(() => expect(restrictSwitch()).not.toHaveAttribute("data-disabled"), { timeout: 5000 });
    expect(restrictSwitch()).toHaveAttribute("aria-checked", "false");
  });

  it("offers a retry when configuration fails to load", async () => {
    let calls = 0;
    const transport: Transport = (url) => {
      calls += 1;
      if (calls === 1) return jsonResponse(url, 500, { detail: "boom" });
      return jsonResponse(url, 200, [configRow("DISABLE_WORKSPACE_CREATION", "1")]);
    };
    createAppClient("https://api.test", transport);
    renderToggle();
    await waitFor(() => expect(screen.getByTestId("admin-workspace-toggle-error")).toBeInTheDocument());
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("admin-workspace-toggle-retry"));
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "true"));
    expect(calls).toBe(2);
  });

  it("seeds from the shared cache without refetching", async () => {
    createAppClient("https://api.test", hangingTransport());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.configurations, [configRow("DISABLE_WORKSPACE_CREATION", "1")] as never);
    renderToggle(queryClient);
    await waitFor(() => expect(restrictSwitch()).toHaveAttribute("aria-checked", "true"));
  });
});
