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
});
