// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace Apps catalog route (CLOUD-021/022). Thin mount: the
// trailing-slash landing, the edition gate, and the connected view
// behind it. Reads fetch on mount — there is deliberately no loader.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { isRedirect } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import type { OpenhubMarketplacePage, Profile } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { edition } from "../../../../core/edition/index.js";
import { sessionKeys } from "../../../../core/session/queries.js";
import { openhubKeys } from "../../../../features/openhub/index.js";
import { Route } from "./index.js";

function seedCatalog(queryClient: QueryClient): void {
  queryClient.setQueryData(openhubKeys.status("acme"), {
    connected: true,
    has_gateway_scopes: true,
    hub_url: "https://hub.test",
  });
  queryClient.setQueryData(openhubKeys.marketplacePage("acme", "", null), {
    items: [{ appId: "app-notes", slug: "notes", name: "Notes Pro" }],
    nextCursor: null,
  } as OpenhubMarketplacePage);
  queryClient.setQueryData(sessionKeys.profile, {
    settings: { openhub: { apps_enabled: false } },
  } as unknown as Profile);
}

afterEach(() => {
  delete edition.routes;
  resetAppClient();
  vi.restoreAllMocks();
});

describe("apps route", () => {
  it("mounts the catalog view for the workspace on cloud editions (CLOUD-022)", () => {
    edition.routes = [{ path: "apps" }];
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    seedCatalog(queryClient);
    const Component = Route.options.component;
    if (!Component) throw new Error("apps route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { name: "Apps", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("searchbox")).toBeInTheDocument();
    expect(screen.getByText("Notes Pro")).toBeInTheDocument();
  });

  it("renders not-found without the edition route (CLOUD-021 flag-off)", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const Component = Route.options.component;
    if (!Component) throw new Error("apps route has no component");
    render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
  });

  it("lands the slashless address on the slashed one (CLOUD-021)", () => {
    try {
      Route.options.beforeLoad?.({
        location: { pathname: "/acme/apps", searchStr: "", hash: "" },
      } as never);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      expect((error as unknown as { options: { href: string } }).options.href).toBe("/acme/apps/");
      return;
    }
    expect.unreachable("expected a redirect");
  });

  it("carries the query string and fragment through the slash landing", () => {
    try {
      Route.options.beforeLoad?.({
        location: { pathname: "/acme/apps", searchStr: "?error=oauth_failed", hash: "card" },
      } as never);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      expect((error as unknown as { options: { href: string } }).options.href).toBe(
        "/acme/apps/?error=oauth_failed#card"
      );
      return;
    }
    expect.unreachable("expected a redirect");
  });

  it("leaves the slashed address alone", () => {
    expect(
      Route.options.beforeLoad?.({
        location: { pathname: "/acme/apps/", searchStr: "", hash: "" },
      } as never)
    ).toBeUndefined();
  });

  it("carries no loader: reads fetch on mount (CLOUD-030 fail-once shape)", () => {
    // A loader prefetch would cache the first failure, and the mount
    // refetch would then consume the oracle's fail-once-then-live
    // second call before the error panel renders (unreachable-status
    // and catalog-failure scenarios); the marketplace window must also
    // only load behind a connected status. The component suites pin
    // both fail-once shapes at mount level — this guards the route
    // against reintroducing the loader.
    expect(Route.options.loader).toBeUndefined();
  });
});
