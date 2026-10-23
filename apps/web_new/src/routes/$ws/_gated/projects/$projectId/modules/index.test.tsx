// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules tab home (parity/specs/modules.md §2.1, MOD-001). The route
// plumbs its params into the modules-owned list screen; the screen suite
// owns the states. Reads are stubbed so the route renders without a server.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Stand-ins for the router: navigation and linking are TanStack's own
// tested behavior; this suite owns the route plumbing.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import { MODULES_PROJECT_ID, makeModuleRow, makeProjectRow } from "../../../../../../features/modules/testing.js";
import { Route } from "./index.js";

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

afterEach(() => {
  resetAppClient();
  document.title = "";
});

describe("modules tab route", () => {
  it("mounts the modules list screen for the route params", async () => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`)) {
        return jsonResponse(url, 200, makeProjectRow());
      }
      if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, { [MODULES_PROJECT_ID]: 20 });
      if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/`)) {
        return jsonResponse(url, 200, [makeModuleRow({ name: "Delivery" })]);
      }
      throw new Error(`unexpected request: ${url}`);
    }) as never);
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: MODULES_PROJECT_ID } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <Component />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("modules-list")).toHaveTextContent("Delivery"));
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Modules");
    expect(screen.queryByRole("heading", { name: "Modules" })).not.toBeInTheDocument();
    expect(document.title).toBe("Website relaunch - Modules");
  });
});
