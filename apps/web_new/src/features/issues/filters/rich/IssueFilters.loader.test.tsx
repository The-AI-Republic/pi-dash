// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filter writes and the floating spinner (ISS-005). The filter control and
// row are built above the layout root, so their writes only raise the
// spinner when the builder and the root share one mutation-loader scope.
// This case mounts the real builder, the real filter row and the real
// layout root, removes a stored condition while the preference PATCH is
// held open, and watches the root's spinner rise and fall.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { sessionKeys } from "../../../../core/session/queries.js";
import { IssuesLayoutRoot } from "../../layouts/IssuesLayoutRoot.js";
import type { PrefsRecord } from "../../prefs/entities.js";
import { prefsQueryOptions } from "../../prefs/prefsQuery.js";
import IssueFilters from "./IssueFilters.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const USER = "123e4567-e89b-12d3-a456-426614174000";
const ENTITY = { scope: "project", projectId: PROJECT } as const;

function prefs(richFilters: Record<string, unknown>): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout: "list", group_by: null, order_by: "sort_order" },
    display_properties: {},
    rich_filters: richFilters,
  };
}

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => (resolve = done));
  return { promise, resolve };
}

function setup() {
  const held = deferred();
  const patches: Array<Record<string, unknown>> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    if (init.method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      patches.push(body);
      await held.promise;
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(prefs((body["rich_filters"] as Record<string, unknown>) ?? {})),
      };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(prefs({ priority__in: "urgent" })),
    };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(prefsQueryOptions({} as never, "acme", ENTITY).queryKey, prefs({ priority__in: "urgent" }));
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: USER, name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: 20 },
  ]);
  queryClient.setQueryData(sessionKeys.me, { id: USER });
  render(
    <QueryClientProvider client={queryClient}>
      <IssueFilters workspaceSlug="acme" projectId={PROJECT} hideCycle hideModule viewsEnabled={false}>
        {(parts) => (
          <div>
            <div data-testid="header-control">{parts.control}</div>
            {parts.row}
            <React.Suspense fallback={<p>suspended</p>}>
              <IssuesLayoutRoot
                workspaceSlug="acme"
                entity={ENTITY}
                title="Website"
                totalCount={1}
                resultCount={1}
                offered={["list"]}
                renderLayout={() => <p>list rows</p>}
                emptyState={<p>Nothing here yet</p>}
                noMatchState={<p>No issues match</p>}
              />
            </React.Suspense>
          </div>
        )}
      </IssueFilters>
    </QueryClientProvider>
  );
  return { held, patches };
}

afterEach(() => {
  resetAppClient();
});

describe("filter writes above the layout root (ISS-005)", () => {
  it("raises the floating spinner while the write and refetch settle", async () => {
    const user = userEvent.setup();
    const { held, patches } = setup();
    const row = await screen.findByTestId("rich-filter-row");
    expect(await screen.findByText("list rows")).toBeInTheDocument();
    expect(screen.queryByTestId("issues-mutation-spinner")).not.toBeInTheDocument();

    await user.click(within(row).getByRole("button", { name: "Remove filter" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    await waitFor(() => expect(screen.getByTestId("issues-mutation-spinner")).toBeInTheDocument());

    held.resolve();
    await waitFor(() => expect(screen.queryByTestId("issues-mutation-spinner")).not.toBeInTheDocument());
    expect(patches[0]).toEqual({ rich_filters: {} });
  });
});
