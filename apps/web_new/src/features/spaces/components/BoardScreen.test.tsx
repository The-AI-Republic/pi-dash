// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardSettings, Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({
  client: null as unknown,
  queryClient: null as unknown,
}));

const navigation = vi.hoisted(() => ({ navigate: vi.fn() }));
const embed = vi.hoisted(() => ({ framed: false }));

vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));
vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigation.navigate }));
vi.mock("../lib/spaceEmbed.js", () => ({ useSpaceEmbedded: () => embed.framed }));

import { createClient } from "@pidash/api-client";

import type { SpaceBoardSearch } from "../lib/address.js";
import { SPACE_HEAD_DESCRIPTION_FALLBACK, SPACE_HEAD_TITLE_FALLBACK } from "../lib/boardHead.js";
import { BoardScreen, boardFiltersOf } from "./BoardScreen.js";

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
  view_props: { list: true, kanban: true, calendar: true, gantt: true, spreadsheet: true },
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

function renderBoard(routes: Routes = {}, anchor = ANCHOR, search: SpaceBoardSearch = {}): string[] {
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
      <BoardScreen anchor={anchor} search={search} />
    </QueryClientProvider>
  );
  return requests;
}

afterEach(() => {
  document.title = "";
  for (const tag of [...document.head.querySelectorAll("meta")]) tag.remove();
  navigation.navigate.mockReset();
  embed.framed = false;
  vi.restoreAllMocks();
});

function expectGenericFallbackHead(titlePart: string): void {
  expect(document.title).toContain(titlePart);
  expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  expect(document.head.querySelector('meta[name="description"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_DESCRIPTION_FALLBACK
  );
  expect(document.head.querySelector('meta[property="og:title"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_TITLE_FALLBACK
  );
  expect(document.head.querySelector('meta[property="og:description"]')).toHaveAttribute(
    "content",
    SPACE_HEAD_DESCRIPTION_FALLBACK
  );
  expect(document.head.querySelector('meta[property="og:image"]')).toBeNull();
  expect(document.head.querySelector('meta[name="twitter:image"]')).toBeNull();
}

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

  it("keeps the generic description and social tags on a malformed anchor", () => {
    renderBoard({}, "bad anchor");
    expect(screen.getByTestId("space-anchor-not-found")).toBeInTheDocument();
    expectGenericFallbackHead("Board not found");
  });

  it("keeps the generic description and social tags on an unknown anchor", async () => {
    renderBoard({ settings: () => reply(404, { detail: "missing" }) });
    expect(await screen.findByTestId("space-anchor-not-found")).toBeInTheDocument();
    expectGenericFallbackHead("Board not found");
  });

  it("keeps the generic description and social tags on a settings failure", async () => {
    renderBoard({ settings: () => reply(500, { detail: "down" }) });
    expect(await screen.findByTestId("space-board-error")).toBeInTheDocument();
    expectGenericFallbackHead("Board error");
  });

  it("keeps the generic description and social tags on an issues failure", async () => {
    renderBoard({ issues: () => reply(500, { detail: "down" }) });
    expect(await screen.findByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board");
    expectGenericFallbackHead("Board error");
  });

  it("renders the list layout grouped by state with the public key once data settles", async () => {
    renderBoard({}, ANCHOR, { board: "list" });
    expect(await screen.findByText("Anonymous visitors can read this")).toBeInTheDocument();
    expect(screen.getByText("SPC-7")).toBeInTheDocument();
    expect(screen.getByTestId("space-list-layout")).toBeInTheDocument();
    expect(screen.getByTestId("space-group-name")).toHaveTextContent("Todo");
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dlist"
    );
  });

  it("renders the board layout from a pasted address", async () => {
    renderBoard({}, ANCHOR, { board: "kanban" });
    expect(await screen.findByTestId("space-board-layout")).toBeInTheDocument();
    expect(screen.getByText("Anonymous visitors can read this")).toBeInTheDocument();
    expect(screen.queryByTestId("space-list-layout")).not.toBeInTheDocument();
  });

  it("offers exactly the publisher-enabled layouts and settles a missing view on list", async () => {
    renderBoard({}, ANCHOR, {});
    const switcher = await screen.findByTestId("space-layout-switcher");
    const options = within(switcher).getAllByTestId("space-layout-option");
    expect(options.map((option) => option.getAttribute("data-layout"))).toEqual(["list", "kanban"]);
    await waitFor(() =>
      expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=list", replace: true })
    );
  });

  it("offers only what the publisher enabled even when other views are on", async () => {
    renderBoard(
      { settings: () => reply(200, { ...settings, view_props: { kanban: true, calendar: true } }) },
      ANCHOR,
      {}
    );
    const options = await screen.findAllByTestId("space-layout-option");
    expect(options.map((option) => option.getAttribute("data-layout"))).toEqual(["kanban"]);
    expect(await screen.findByTestId("space-board-layout")).toBeInTheDocument();
  });

  it("switches layout without fetching the issues again, keeping filters and peek", async () => {
    const requests = renderBoard({}, ANCHOR, { board: "list", peekId: "p1", priority: ["high"] });
    await screen.findByTestId("space-list-layout");
    const issuesBefore = requests.filter((url) => url.includes("/issues/")).length;
    await userEvent.click(screen.getByRole("button", { name: "Board layout" }));
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=kanban&peekId=p1&priority=high",
    });
    expect(requests.filter((url) => url.includes("/issues/")).length).toBe(issuesBefore);
  });

  it("does not rewrite the address when it already names an offered layout", async () => {
    renderBoard({}, ANCHOR, { board: "kanban" });
    await screen.findByTestId("space-board-layout");
    expect(navigation.navigate).not.toHaveBeenCalled();
  });

  it("sends the address filters on the board's first fetch", async () => {
    const state = "44444444-4444-4444-8444-444444444444";
    const requests = renderBoard({}, ANCHOR, { board: "list", state: [state], priority: ["low"] });
    await screen.findByTestId("space-list-layout");
    const issues = requests.filter((url) => url.includes("/issues/"));
    expect(issues).toHaveLength(1);
    const url = new URL(issues[0] ?? "", "https://api.test");
    expect(url.searchParams.get("state")).toBe(state);
    expect(url.searchParams.get("priority")).toBe("low");
    expect(url.searchParams.get("cursor")).toBe("50:0:0");
    expect(url.searchParams.get("group_by")).toBe("state_id");
  });

  it("omits the project key prefix when settings carry no project details", async () => {
    renderBoard({ settings: () => reply(200, { ...settings, project_details: null }) }, ANCHOR, { board: "list" });
    expect(await screen.findByText("Anonymous visitors can read this")).toBeInTheDocument();
    expect(screen.getByText("-7")).toBeInTheDocument();
  });

  it("shows the dedicated empty notice on a zero-issue board in both layouts", async () => {
    const empty = {
      grouped_by: "state_id",
      total_count: 0,
      next_cursor: null,
      next_page_results: false,
      results: {},
    };
    renderBoard({ issues: () => reply(200, empty) }, ANCHOR, { board: "list" });
    expect(await screen.findByTestId("space-board-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("space-list-layout")).not.toBeInTheDocument();
    expect(screen.getByTestId("space-layout-switcher")).toBeInTheDocument();
  });

  it("shows the empty notice in the board layout too", async () => {
    renderBoard(
      {
        issues: () =>
          reply(200, {
            grouped_by: "state_id",
            total_count: 0,
            next_cursor: null,
            next_page_results: false,
            results: {},
          }),
      },
      ANCHOR,
      { board: "kanban" }
    );
    expect(await screen.findByTestId("space-board-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("space-board-layout")).not.toBeInTheDocument();
  });

  it("mounts the product badge once issues and states settle (SPC-012)", async () => {
    renderBoard();
    expect(await screen.findByTestId("space-product-badge")).toBeInTheDocument();
  });

  it("mounts no badge while settings are still loading", async () => {
    renderBoard({ settings: () => new Promise(() => undefined) });
    expect(screen.getByTestId("space-board-placeholder")).toBeInTheDocument();
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
  });

  it("mounts no badge while issues load or fail under a resolved navbar", async () => {
    renderBoard({ issues: () => new Promise(() => undefined) });
    expect(await screen.findByTestId("space-board-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
  });

  it("mounts no badge on an issues failure", async () => {
    renderBoard({ issues: () => reply(500, { detail: "down" }) });
    expect(await screen.findByTestId("space-board-error")).toBeInTheDocument();
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
  });

  it("mounts no badge inside an embedding frame", async () => {
    embed.framed = true;
    renderBoard();
    expect(await screen.findByText("Anonymous visitors can read this")).toBeInTheDocument();
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
  });

  it("shows the Filters control once states settle and hides it while they load (SPC-034)", async () => {
    renderBoard({ states: () => new Promise(() => undefined) });
    expect(await screen.findByTestId("space-board-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("space-filters-button")).not.toBeInTheDocument();
  });

  it("pushes a picked panel value under its singular key (SPC-034/035)", async () => {
    renderBoard({}, ANCHOR, { board: "list" });
    await screen.findByTestId("space-list-layout");
    await userEvent.click(screen.getByTestId("space-filters-button"));
    await screen.findByTestId("space-filters-panel");
    const high = screen.getAllByTestId("space-filter-option").find((row) => row.getAttribute("data-value") === "high");
    if (!high) throw new Error("missing High option");
    await userEvent.click(high);
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=list&priority=high",
    });
  });

  it("renders applied pills from the address and removes one through its pill (SPC-035)", async () => {
    renderBoard({}, ANCHOR, { board: "list", priority: ["high", "urgent"] });
    await screen.findByTestId("space-list-layout");
    const strip = screen.getByTestId("space-applied-filters");
    expect(within(strip).getAllByTestId("space-applied-pill")).toHaveLength(2);
    const remove = within(strip)
      .getAllByTestId("space-applied-remove")
      .find((entry) => entry.getAttribute("data-value") === "high");
    if (!remove) throw new Error("missing High remove");
    await userEvent.click(remove);
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=list&priority=urgent",
    });
  });

  it("clears every facet through the strip and keeps the view and the open issue (SPC-035/036)", async () => {
    renderBoard({}, ANCHOR, { board: "kanban", peekId: "p1", priority: ["high"], state: [STATE_ID] });
    await screen.findByTestId("space-board-layout");
    expect(screen.getByTestId("space-applied-filters")).toBeInTheDocument();
    await userEvent.click(screen.getByTestId("space-applied-clear-all"));
    expect(navigation.navigate).toHaveBeenCalledWith({
      href: "/spaces/issues/anchor-1/?board=kanban&peekId=p1",
    });
  });

  it("sends an address-only label filter on the first fetch and shells it without pills (SPC-037)", async () => {
    const label = "55555555-5555-4555-8555-555555555555";
    const requests = renderBoard({}, ANCHOR, { board: "list", labels: [label] });
    await screen.findByTestId("space-list-layout");
    const issues = requests.filter((url) => url.includes("/issues/"));
    expect(issues).toHaveLength(1);
    expect(new URL(issues[0] ?? "", "https://api.test").searchParams.get("labels")).toBe(label);
    const shells = within(screen.getByTestId("space-applied-filters")).getAllByTestId("space-applied-facet");
    expect(shells.map((entry) => entry.getAttribute("data-facet"))).toEqual(["labels"]);
    expect(screen.queryByTestId("space-applied-pill")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("space-applied-clear"));
    expect(navigation.navigate).toHaveBeenCalledWith({ href: "/spaces/issues/anchor-1/?board=list" });
  });

  it("renders no pill for a state id the states fetch does not know, but still sends it (SPC-035/036)", async () => {
    const unknown = "66666666-6666-4666-8666-666666666666";
    const requests = renderBoard({}, ANCHOR, { board: "list", state: [unknown] });
    await screen.findByTestId("space-list-layout");
    const issues = requests.filter((url) => url.includes("/issues/"));
    expect(new URL(issues[0] ?? "", "https://api.test").searchParams.get("state")).toBe(unknown);
    expect(screen.queryByTestId("space-applied-pill")).not.toBeInTheDocument();
    expect(screen.getByTestId("space-applied-clear")).toBeInTheDocument();
  });

  it("offers no text search on the board while the panel is closed (SPC-039)", async () => {
    renderBoard({}, ANCHOR, { board: "list" });
    await screen.findByTestId("space-list-layout");
    expect(screen.queryAllByRole("textbox")).toEqual([]);
  });
});

describe("boardFiltersOf", () => {
  it("keeps only the facets the fetch consumes", () => {
    expect(boardFiltersOf({ board: "kanban", peekId: "p1" })).toEqual({});
    const state = "22222222-2222-4222-8222-222222222222";
    const label = "33333333-3333-4333-8333-333333333333";
    expect(boardFiltersOf({ state: [state], priority: ["high"], labels: [label] })).toEqual({
      state: [state],
      priority: ["high"],
      labels: [label],
    });
  });

  it("reads raw address strings and drops malformed values", () => {
    const state = "22222222-2222-4222-8222-222222222222";
    const raw = { state: `${state},nope`, priority: "high,unset" } as unknown as SpaceBoardSearch;
    expect(boardFiltersOf(raw)).toEqual({ state: [state], priority: ["high"] });
  });
});
