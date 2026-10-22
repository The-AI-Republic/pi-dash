// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list route (parity/specs/views.md §9, VIEW-029). The route
// plumbs its workspace param into the views-owned screen; the screen suite
// owns the states. Reads are stubbed so the route renders without a server.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { makeView } from "../../../../features/views/testing.js";
import { Route } from "./index.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

beforeEach(() => {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.includes("/api/workspaces/acme/views/"))
      return jsonResponse(url, 200, [makeView({ name: "Triage board" })]);
    if (url.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(url, 200, [
        { id: WS_ID, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
      ]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
  vi.restoreAllMocks();
});

describe("workspace views list route", () => {
  it("mounts the views-owned list screen for the workspace param", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Component />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("workspace-views-customs")).toBeInTheDocument());
    expect(screen.getByTestId("workspace-views-customs")).toHaveTextContent("Triage board");
    expect(screen.getByTestId("workspace-views-defaults")).toHaveTextContent("All work items");
    expect(document.title).toBe("Acme - All Views");
  });
});
