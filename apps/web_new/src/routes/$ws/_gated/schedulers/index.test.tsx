// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scheduler catalog route (AGT-001). Proves prefetch and workspace mount.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import type { Scheduler } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../../core/session/index.js";
import { sessionKeys } from "../../../../core/session/queries.js";
import { Route } from "./index.js";

const ROW: Scheduler = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  workspace: "223e4567-e89b-12d3-a456-426614174000",
  slug: "weekly-report",
  name: "Weekly report",
  description: "Summary",
  prompt: "Review",
  color: "#5e6ad2",
  source: "builtin",
  is_enabled: true,
  active_binding_count: 2,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("schedulers route", () => {
  it("prefetches the workspace scheduler list", async () => {
    const ensureQueryData = vi.fn().mockResolvedValue([ROW]);
    const loader = Route.options.loader as unknown as (input: unknown) => Promise<unknown>;
    await loader({
      context: { queryClient: { ensureQueryData }, apiClient: {} },
      params: { ws: "acme" },
    });
    expect(ensureQueryData).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: ["ws", "acme", "schedulers", "list"],
      })
    );
  });

  it("mounts the catalog for the route workspace", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["ws", "acme", "schedulers", "list"], [ROW]);
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: "w1", name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: ROLE_ADMIN },
    ]);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("heading", { name: "Schedulers" })).toBeInTheDocument();
    expect(screen.getByText("Weekly report")).toBeInTheDocument();
  });
});
