// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../../../core/api/client.js";
import { sessionKeys } from "../../../../core/session/queries.js";
import { IssuesLoaderProvider } from "../../loaders/loaderContext.js";
import type { PrefsRecord } from "../../prefs/entities.js";
import { prefsQueryOptions } from "../../prefs/prefsQuery.js";
import IssueFilters, { type IssueFiltersProps } from "./IssueFilters.js";
import type { RichFilterBuilderParts } from "./RichFilterBuilder.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const USER = "123e4567-e89b-12d3-a456-426614174000";

function emptyRecord(): PrefsRecord {
  return {
    filters: {},
    display_filters: { layout: "list" },
    display_properties: {},
    rich_filters: {},
  };
}

function setup(
  overrides: Partial<IssueFiltersProps> = {},
  richFilters: Record<string, unknown> = {}
): RichFilterBuilderParts {
  const record = { ...emptyRecord(), rich_filters: richFilters };
  createAppClient("https://api.test", (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(record),
  })) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(
    prefsQueryOptions({} as never, "acme", { scope: "project", projectId: PROJECT }).queryKey,
    record
  );
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: USER, name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: 20 },
  ]);
  queryClient.setQueryData(sessionKeys.me, { id: USER });
  const capture = vi.fn<(parts: RichFilterBuilderParts) => void>();
  render(
    <QueryClientProvider client={queryClient}>
      <IssuesLoaderProvider>
        <IssueFilters
          workspaceSlug="acme"
          projectId={PROJECT}
          hideCycle={true}
          hideModule={true}
          viewsEnabled={false}
          children={(parts) => {
            capture(parts);
            return (
              <div>
                <span data-testid="control-slot">{parts.control}</span>
                <span data-testid="instance-id">{parts.instanceId}</span>
                {parts.row}
              </div>
            );
          }}
          {...overrides}
        />
      </IssuesLoaderProvider>
    </QueryClientProvider>
  );
  return capture.mock.lastCall![0];
}

describe("IssueFilters", () => {
  it("hands the mounted builder's control, row and instance id to the route", () => {
    const parts = setup();
    expect(screen.getByTestId("instance-id")).toHaveTextContent(`project:${PROJECT}`);
    expect(parts.instanceId).toBe(`project:${PROJECT}`);
    // The empty expression renders the add control as the header entry.
    expect(screen.getByTestId("control-slot")).toContainElement(screen.getByTestId("rich-filter-add"));
  });

  it("threads the views switch down to the row actions", async () => {
    setup({ viewsEnabled: true }, { priority__in: "urgent" });
    expect(await screen.findByTestId("rich-save-view")).toBeInTheDocument();
    expect(screen.getByTestId("rich-clear-all")).toBeInTheDocument();
  });

  it("withholds saving when the project views feature is off", async () => {
    setup({ viewsEnabled: false }, { priority__in: "urgent" });
    expect(await screen.findByTestId("rich-clear-all")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument());
  });

  it("keeps a saved view's columns stable when the row re-renders", async () => {
    const view = {
      id: "323e4567-e89b-12d3-a456-426614174002",
      workspace: USER,
      project: PROJECT,
      name: "Mine",
      description: "",
      query: {},
      filters: {},
      display_filters: { layout: "list" },
      display_properties: {},
      rich_filters: { priority__in: "urgent" },
      access: 0,
      sort_order: 1,
      logo_props: {},
      owned_by: USER,
      is_locked: false,
      archived_at: null,
      is_favorite: false,
      created_by: USER,
      updated_by: null,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    };
    createAppClient("https://api.test", (() => {
      throw new Error("a seeded view render reads nothing over the network");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: USER, name: "Acme", slug: "acme", logo_url: null, total_members: 1, role: 20 },
    ]);
    queryClient.setQueryData(sessionKeys.me, { id: USER });
    const capture = vi.fn<(parts: RichFilterBuilderParts) => void>();
    function Screen({ viewRecord }: { viewRecord: typeof view }): React.ReactElement {
      return (
        <QueryClientProvider client={queryClient}>
          <IssuesLoaderProvider>
            <IssueFilters
              workspaceSlug="acme"
              projectId={PROJECT}
              hideCycle={true}
              hideModule={true}
              viewsEnabled={true}
              view={viewRecord}
              children={(parts) => {
                capture(parts);
                return <div data-testid="row-slot">{parts.row}</div>;
              }}
            />
          </IssuesLoaderProvider>
        </QueryClientProvider>
      );
    }
    const { rerender } = render(<Screen viewRecord={view} />);
    const before = capture.mock.calls.length;
    rerender(<Screen viewRecord={view} />);
    // A re-render with the same record must settle, not reset on every pass.
    await waitFor(() => expect(capture.mock.calls.length).toBeGreaterThan(before));
    expect(screen.getByTestId("row-slot")).toHaveTextContent("Urgent");
  });

  it("edits the entity it is handed instead of the project record (ISS-071)", () => {
    const cycle = { scope: "cycle", projectId: PROJECT, cycleId: "323e4567-e89b-12d3-a456-426614174002" } as const;
    const parts = setup({ entity: cycle });
    expect(parts.instanceId).toBe(`cycle:${PROJECT}/${cycle.cycleId}`);
  });

  it("reads the project record when no entity is passed", () => {
    const parts = setup();
    expect(parts.instanceId).toBe(`project:${PROJECT}`);
  });
});
