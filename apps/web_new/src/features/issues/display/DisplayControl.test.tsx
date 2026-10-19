// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { IssuesLoaderProvider } from "../loaders/loaderContext.js";
import type { LayoutKind } from "../layouts/kinds.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import { parseDisplayPrefs, type DisplayPrefs } from "../prefs/displayPrefs.js";
import { DisplayControl } from "./DisplayControl.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT_ENTITY: IssuesEntity = { scope: "project", projectId: PROJECT_ID };
const EPICS_ENTITY: IssuesEntity = { scope: "epics", projectId: PROJECT_ID };
const OFFERED: readonly LayoutKind[] = ["list", "kanban", "calendar", "spreadsheet", "gantt_chart"];

interface PatchCall {
  url: string;
  body: { display_filters?: Record<string, unknown>; display_properties?: Record<string, unknown> };
}

function prefsRecord(displayFilters: Record<string, unknown>, displayProperties: Record<string, unknown>) {
  return { filters: {}, display_filters: displayFilters, display_properties: displayProperties, rich_filters: {} };
}

function renderControl(options: {
  entity?: IssuesEntity;
  offered?: readonly LayoutKind[];
  seedFilters?: Record<string, unknown>;
  seedProperties?: Record<string, unknown>;
  hang?: boolean;
}): { patches: PatchCall[]; queryClient: QueryClient } {
  const {
    entity = PROJECT_ENTITY,
    offered = OFFERED,
    seedFilters = { layout: "list", group_by: null, order_by: "sort_order" },
    seedProperties = {},
    hang = false,
  } = options;
  const patches: PatchCall[] = [];
  const apiClient = createAppClient("https://api.test", (async (
    url: string,
    init: { method?: string; body?: string }
  ) => {
    if (hang) await new Promise(() => {});
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (init.method === "PATCH") {
      const body = JSON.parse(init.body ?? "{}") as {
        display_filters?: Record<string, unknown>;
        display_properties?: Record<string, unknown>;
      };
      patches.push({ url, body });
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            id: "123e4567-e89b-12d3-a456-426614174000",
            workspace: "123e4567-e89b-12d3-a456-426614174000",
            project: PROJECT_ID,
            user: "123e4567-e89b-12d3-a456-426614174000",
            filters: {},
            display_filters: body.display_filters ?? {},
            display_properties: body.display_properties ?? {},
            rich_filters: {},
            preferences: {},
            sort_order: 1,
            created_by: null,
            updated_by: null,
            created_at: "2026-10-01T00:00:00Z",
            updated_at: "2026-10-01T00:00:00Z",
          }),
      };
    }
    throw new Error(`unexpected ${init.method ?? "GET"} ${url} in a seeded render`);
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (!hang) {
    queryClient.setQueryData(
      prefsQueryOptions(apiClient, "acme", entity).queryKey,
      prefsRecord(seedFilters, seedProperties)
    );
  }
  render(
    <QueryClientProvider client={queryClient}>
      <IssuesLoaderProvider>
        <DisplayControl workspaceSlug="acme" entity={entity} offered={offered} hideCycle={false} hideModule={false} />
      </IssuesLoaderProvider>
    </QueryClientProvider>
  );
  return { patches, queryClient };
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

async function openPanel(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
  await user.click(screen.getByTestId("display-options-trigger"));
  return await screen.findByTestId("display-options-panel");
}

describe("DisplayControl", () => {
  it("opens the list panel and toggles a pill through a server patch", async () => {
    const user = userEvent.setup();
    const { patches } = renderControl({});
    const panel = await openPanel(user);
    for (const heading of ["Display Properties", "Group by", "Order by", "Options"]) {
      expect(within(panel).getByRole("heading", { name: heading })).toBeInTheDocument();
    }
    expect(within(panel).queryByRole("heading", { name: "Sub-group by" })).not.toBeInTheDocument();

    const pill = within(panel).getByRole("button", { name: "Assignee" });
    expect(pill).toHaveAttribute("aria-pressed", "true");
    await user.click(pill);
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]!.url).toContain(`/api/workspaces/acme/projects/${PROJECT_ID}/user-properties/`);
    expect(patches[0]!.body.display_properties?.["assignee"]).toBe(false);
    expect(within(panel).getByRole("button", { name: "Assignee" })).toHaveAttribute("aria-pressed", "false");
  });

  it("persists group by through a server patch", async () => {
    const user = userEvent.setup();
    const { patches } = renderControl({});
    const panel = await openPanel(user);
    await user.click(within(panel).getByRole("button", { name: "States" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]!.body.display_filters?.["group_by"]).toBe("state");
    expect(within(panel).getByRole("button", { name: "States" })).toHaveAttribute("aria-pressed", "true");
  });

  it("persists sub-group by and order by on kanban", async () => {
    const user = userEvent.setup();
    const { patches } = renderControl({
      seedFilters: { layout: "kanban", group_by: "state", sub_group_by: null, order_by: "sort_order" },
    });
    const panel = await openPanel(user);
    expect(within(panel).getByRole("heading", { name: "Sub-group by" })).toBeInTheDocument();
    const subSection = within(panel).getByTestId("display-section-subGroup");
    await user.click(within(subSection).getByRole("button", { name: "Priority" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]!.body.display_filters?.["sub_group_by"]).toBe("priority");

    const orderSection = within(panel).getByTestId("display-section-order");
    await user.click(within(orderSection).getByRole("button", { name: "Last created" }));
    await waitFor(() => expect(patches).toHaveLength(2));
    expect(patches[1]!.body.display_filters?.["order_by"]).toBe("-created_at");
  });

  it("persists both switches through server patches", async () => {
    const user = userEvent.setup();
    const { patches } = renderControl({});
    const panel = await openPanel(user);
    const options = within(panel).getByTestId("display-section-options");
    const empties = within(options).getByRole("button", { name: "Show empty groups" });
    const subItems = within(options).getByRole("button", { name: "Show sub-work items" });
    expect(empties).toHaveAttribute("aria-pressed", "false");
    expect(subItems).toHaveAttribute("aria-pressed", "false");

    await user.click(empties);
    await waitFor(() => expect(patches).toHaveLength(1));
    expect(patches[0]!.body.display_filters?.["show_empty_groups"]).toBe(true);

    await user.click(subItems);
    await waitFor(() => expect(patches).toHaveLength(2));
    expect(patches[1]!.body.display_filters?.["sub_issue"]).toBe(true);
  });

  it("resolves the panel matrix to the offered layout, not the stored one", async () => {
    const user = userEvent.setup();
    renderControl({
      offered: ["list"],
      seedFilters: { layout: "kanban", group_by: "state", order_by: "sort_order" },
    });
    const panel = await openPanel(user);
    expect(within(panel).queryByRole("heading", { name: "Sub-group by" })).not.toBeInTheDocument();
    // The seeded group is applied, so its title carries the count (ISS-094).
    expect(within(panel).getByRole("heading", { name: "Group by (1)" })).toBeInTheDocument();
  });

  it("shows a loading note while preferences resolve", async () => {
    const user = userEvent.setup();
    renderControl({ hang: true });
    await user.click(screen.getByTestId("display-options-trigger"));
    expect(await screen.findByText("Loading display options…")).toBeInTheDocument();
    expect(screen.queryByTestId("display-options-panel")).not.toBeInTheDocument();
  });

  it("relabels the count pill on the epics entity", async () => {
    const user = userEvent.setup();
    renderControl({ entity: EPICS_ENTITY });
    const panel = await openPanel(user);
    expect(within(panel).getByRole("button", { name: "Work item count" })).toBeInTheDocument();
  });

  it("never touches the URL while display writes land (ISS-106)", async () => {
    const user = userEvent.setup();
    const before = window.location.href;
    const { patches } = renderControl({});
    const panel = await openPanel(user);
    await user.click(within(panel).getByRole("button", { name: "States" }));
    await waitFor(() => expect(patches).toHaveLength(1));
    await user.click(within(panel).getByRole("button", { name: "Assignee" }));
    await waitFor(() => expect(patches).toHaveLength(2));
    expect(window.location.href).toBe(before);
  });

  it("edits a supplied saved-view display locally without reading or writing personal preferences", async () => {
    const user = userEvent.setup();
    const changes: DisplayPrefs[] = [];
    // No prefs record is seeded: the personal read stays disabled, so any
    // fetch would hit the throwing transport.
    createAppClient("https://api.test", (() => {
      throw new Error("unexpected network fetch for a saved view's display");
    }) as never);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <IssuesLoaderProvider>
          <DisplayControl
            workspaceSlug="acme"
            entity={PROJECT_ENTITY}
            offered={OFFERED}
            hideCycle={false}
            hideModule={false}
            display={parseDisplayPrefs({ layout: "list", order_by: "sort_order" }, {})}
            onDisplayChange={(next) => changes.push(next)}
          />
        </IssuesLoaderProvider>
      </QueryClientProvider>
    );
    const panel = await openPanel(user);
    await user.click(within(panel).getByRole("button", { name: "States" }));
    expect(changes).toHaveLength(1);
    expect(changes[0]).toMatchObject({ groupBy: "state", layout: "list", orderBy: "sort_order" });
  });
});
