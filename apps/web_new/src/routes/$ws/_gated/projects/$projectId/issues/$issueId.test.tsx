// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  isRedirect,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import type { RouterContext } from "../../../../../__root.js";
import { createAppClient, resetAppClient } from "../../../../../../core/api/client.js";
import { Route } from "./$issueId.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";

function stubClient(metaStatus: number) {
  return createAppClient("https://api.test", async (url: string) => {
    let status = 404;
    let body = "{}";
    if (url.endsWith(`/issues/${ISSUE_ID}/meta/`)) {
      status = metaStatus;
      if (status === 200) {
        body = JSON.stringify({ project_identifier: "WEB", sequence_id: 7 });
      }
    }
    return { status, url, headers: { get: () => null }, text: async () => body };
  });
}

function renderShortLink() {
  stubClient(404);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const component = Route.options.component;
  if (!component) throw new Error("missing component");
  const rootRoute = createRootRouteWithContext<RouterContext>()();
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const gatedRoute = createRoute({ getParentRoute: () => wsRoute, id: "_gated", component: () => <Outlet /> });
  const projectsRoute = createRoute({
    getParentRoute: () => gatedRoute,
    path: "projects/$projectId",
    component: () => <Outlet />,
  });
  const issuesRoute = createRoute({ getParentRoute: () => projectsRoute, path: "issues", component: () => <Outlet /> });
  const shortLinkRoute = createRoute({ getParentRoute: () => issuesRoute, path: "$issueId", component });
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      wsRoute.addChildren([
        gatedRoute.addChildren([projectsRoute.addChildren([issuesRoute.addChildren([shortLinkRoute])])]),
      ]),
    ]),
    context: { queryClient, apiClient: stubClient(404) },
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${PROJECT_ID}/issues/${ISSUE_ID}`] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router };
}

afterEach(() => {
  resetAppClient();
});

describe("short-link route", () => {
  type LoaderInput = { context: { queryClient: QueryClient; apiClient: unknown }; params: unknown };

  function getLoader(): (input: LoaderInput) => Promise<unknown> {
    const loader = Route.options.loader as unknown as (input: LoaderInput) => Promise<unknown>;
    if (typeof loader !== "function") throw new Error("missing loader");
    return loader;
  }

  it("redirects resolved links to the browse key", async () => {
    const apiClient = stubClient(200);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const thrown = await getLoader()({
      context: { queryClient, apiClient },
      params: { ws: "acme", projectId: PROJECT_ID, issueId: ISSUE_ID },
    }).then(
      () => null,
      (error: unknown) => error
    );
    expect(thrown).not.toBeNull();
    expect(isRedirect(thrown)).toBe(true);
    const options = (thrown as unknown as { options?: { to?: unknown; params?: unknown } }).options;
    expect(options?.to).toBe("/$ws/browse/$issueKey");
    expect(options?.params).toEqual({ ws: "acme", issueKey: "WEB-7" });
  });

  it.each([404, 500])("returns the missing marker for status %s", async (metaStatus) => {
    const apiClient = stubClient(metaStatus);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    await expect(
      getLoader()({
        context: { queryClient, apiClient },
        params: { ws: "acme", projectId: PROJECT_ID, issueId: ISSUE_ID },
      })
    ).resolves.toEqual({ missing: true });
  });

  it("renders the missing state with a link to the project list", async () => {
    renderShortLink();
    await screen.findByRole("heading", { name: "No work item matches this address" });
    expect(screen.getByText("It may have been archived or deleted, or the address is wrong.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to the project issues" })).toHaveAttribute(
      "href",
      `/acme/projects/${PROJECT_ID}/issues`
    );
  });

  it("spins while the link resolves", () => {
    const Pending = Route.options.pendingComponent as unknown as React.ComponentType;
    render(<Pending />);
    expect(screen.getByRole("status")).toHaveTextContent("Resolving work item link");
    expect(screen.getByText("Resolving the work item link…")).toBeInTheDocument();
  });
});
