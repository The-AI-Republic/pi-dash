// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardSettings, Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({
  client: null as unknown,
  queryClient: null as unknown,
}));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import { createClient } from "@pidash/api-client";

import { BoardScreen } from "./BoardScreen.js";

const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const STATE_ID = "22222222-2222-4222-8222-222222222222";
const ISSUE_ID = "33333333-3333-4333-8333-333333333333";
const ANCHOR = "anchor-1";

const settings: SpaceBoardSettings = {
  id: "44444444-4444-4444-8444-444444444444",
  project_details: {
    id: PROJECT_ID,
    identifier: "SPC",
    name: "Public board",
    cover_image: null,
    cover_image_url: null,
    logo_props: {},
    description: "",
    is_default: false,
  },
  workspace_detail: { name: "Acme", slug: "acme", id: PROJECT_ID, logo_url: null },
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  deleted_at: null,
  created_by: null,
  updated_by: null,
  workspace: PROJECT_ID,
  project: PROJECT_ID,
  entity_identifier: "project",
  entity_name: "project",
  anchor: ANCHOR,
  is_comments_enabled: false,
  is_reactions_enabled: false,
  is_votes_enabled: false,
  view_props: {},
  is_disabled: false,
  intake: null,
};

const issue = {
  id: ISSUE_ID,
  name: "Anonymous visitors can read this",
  state_id: STATE_ID,
  sort_order: 1,
  estimate_point: null,
  priority: null,
  start_date: null,
  target_date: null,
  sequence_id: 7,
  project_id: PROJECT_ID,
  parent_id: null,
  created_by: null,
  state__group: "unstarted" as const,
  cycle_id: null,
  assignee_ids: [],
  label_ids: [],
  module_ids: [],
  vote_items: [],
  reaction_items: [],
};

const states = [{ id: STATE_ID, name: "Todo", color: "#000", group: "unstarted" as const, sequence: 1 }];

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

interface Routes {
  settings?: () => Promise<Awaited<ReturnType<Transport>>> | Awaited<ReturnType<Transport>>;
  issues?: () => Promise<Awaited<ReturnType<Transport>>> | Awaited<ReturnType<Transport>>;
  states?: () => Promise<Awaited<ReturnType<Transport>>> | Awaited<ReturnType<Transport>>;
}

function renderBoard(routes: Routes = {}, anchor = ANCHOR): string[] {
  const requests: string[] = [];
  const transport: Transport = async (url) => {
    requests.push(url);
    if (url.includes("/settings/")) return await (routes.settings?.() ?? reply(200, settings));
    if (url.includes("/issues/")) {
      return await (routes.issues?.() ??
        reply(200, {
          grouped_by: "state_id",
          total_count: 1,
          next_cursor: null,
          next_page_results: false,
          results: { [STATE_ID]: { results: [issue], total_results: 1 } },
        }));
    }
    if (url.includes("/states/")) return await (routes.states?.() ?? reply(200, states));
    throw new Error(`unexpected request ${url}`);
  };
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <BoardScreen anchor={anchor} search={{}} />
    </QueryClientProvider>
  );
  return requests;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("BoardScreen", () => {
  it("renders the anchor illustration for a malformed anchor without a request", () => {
    const requests = renderBoard({}, "bad anchor");
    expect(screen.getByTestId("space-anchor-not-found")).toBeInTheDocument();
    expect(requests).toEqual([]);
  });

  it("shows placeholder chrome with an empty body while settings load", async () => {
    renderBoard({ settings: () => new Promise(() => undefined) });
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("...");
    expect(screen.getByTestId("space-board-placeholder")).toBeInTheDocument();
    expect(screen.queryByTestId("space-board-issue")).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("classifies a settings 404 as the anchor illustration", async () => {
    renderBoard({ settings: () => reply(404, { detail: "missing" }) });
    expect(await screen.findByTestId("space-anchor-not-found")).toBeInTheDocument();
    expect(screen.queryByTestId("space-navbar-name")).not.toBeInTheDocument();
  });

  it("renders the board-error illustration without chrome for other settings failures", async () => {
    renderBoard({ settings: () => reply(500, { detail: "down" }) });
    expect(await screen.findByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.queryByTestId("space-navbar-name")).not.toBeInTheDocument();
  });

  it("keeps the resolved navbar and shows a spinner while issues and states load", async () => {
    renderBoard({
      issues: () => new Promise(() => undefined),
      states: () => new Promise(() => undefined),
    });
    expect(await screen.findByTestId("space-board-loading")).toBeInTheDocument();
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board");
  });

  it("renders the board-error illustration under the navbar on an issues failure", async () => {
    renderBoard({ issues: () => reply(500, { detail: "down" }) });
    expect(await screen.findByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board");
  });

  it("renders the board-error illustration under the navbar on a states failure", async () => {
    renderBoard({ states: () => reply(500, { detail: "down" }) });
    expect(await screen.findByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board");
  });

  it("renders grouped issue rows with the public key once data settles", async () => {
    renderBoard();
    expect(await screen.findByText("Anonymous visitors can read this")).toBeInTheDocument();
    expect(screen.getByText("SPC-7")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Todo" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/spaces/?next_path=/issues/anchor-1/"
    );
  });

  it("drops empty state groups instead of rendering empty sections", async () => {
    renderBoard({
      issues: () =>
        reply(200, {
          grouped_by: "state_id",
          total_count: 0,
          next_cursor: null,
          next_page_results: false,
          results: { [STATE_ID]: { results: [], total_results: 0 } },
        }),
    });
    await waitFor(() => expect(screen.getByTestId("space-board-issues")).toBeInTheDocument());
    expect(screen.queryByRole("heading", { name: "Todo" })).not.toBeInTheDocument();
  });
});
