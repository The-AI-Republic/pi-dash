// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Notifications inbox route (NTF-001). Thin mount: proves the route
// renders the connected inbox view for the workspace slug.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../../core/session/index.js";
import { sessionKeys } from "../../../../core/session/queries.js";
import { Route } from "./index.js";

beforeAll(async () => {
  await import("../../../../features/notifications/components/InboxView.js");
}, 60_000);

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("notifications route", () => {
  it("declares no shareable selection, tab or filter search state (NTF-027)", () => {
    expect(Route.options).not.toHaveProperty("validateSearch");
  });

  it("mounts the inbox view for the workspace", async () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.workspaces, [
      {
        id: "123e4567-e89b-12d3-a456-426614174000",
        name: "Acme",
        slug: "acme",
        logo_url: null,
        total_members: 1,
        role: ROLE_ADMIN,
      },
    ]);
    const Component = Route.options.component;
    if (!Component) throw new Error("notifications route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("region", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Details" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "All" })).toBeInTheDocument();
  });
});
