// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Favorite, SavedView } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FAVORITES_SECTION_OPEN_KEY } from "../../../shared/shell/favorites.js";
import { useFavorites } from "../../sidebar/index.js";
import { makeView } from "../testing.js";
import { viewKeys } from "./keys.js";
import {
  resetViewFavoriteIds,
  selectPendingViewId,
  useToggleViewFavorite,
  type ToggleViewFavoriteInput,
} from "./viewFavorites.js";

const VIEW_ID = "223e4567-e89b-12d3-a456-426614174001";
const FAVORITE_ID = "723e4567-e89b-12d3-a456-426614174006";
const WORKSPACE_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "323e4567-e89b-12d3-a456-426614174002";

function favoriteRow(): Favorite {
  return {
    id: FAVORITE_ID,
    entity_type: "view",
    entity_identifier: VIEW_ID,
    entity_data: null,
    name: "Triage",
    is_folder: false,
    sequence: 10000,
    parent: null,
    workspace_id: WORKSPACE_ID,
    project_id: PROJECT_ID,
  };
}

interface StubOptions {
  calls?: string[];
  /** Status the favorites write (POST/DELETE) answers. */
  writeStatus?: number;
  /** Rows the favorites list read answers. */
  favorites?: Favorite[];
  /** Holds the write until released, so the pending state is observable. */
  gate?: Promise<void>;
}

function stubClient(options: StubOptions = {}): string[] {
  const calls = options.calls ?? [];
  const writeStatus = options.writeStatus ?? 201;
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    calls.push(`${init?.method ?? "GET"} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/user-favorites/") && (init?.method ?? "GET") === "GET") {
      if (options.gate !== undefined) await options.gate;
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(options.favorites ?? []),
      };
    }
    if (url.includes("/user-favorites/")) {
      if (options.gate !== undefined) await options.gate;
      if (writeStatus >= 400) {
        return {
          status: writeStatus,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ detail: "favorites are unavailable" }),
        };
      }
      const created = (init?.method ?? "GET") === "POST";
      return {
        status: created ? 201 : 204,
        url,
        headers: { get: () => null },
        text: async () => (created ? JSON.stringify(favoriteRow()) : ""),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

function Probe({ row }: { row: SavedView }): React.ReactElement {
  const toggle = useToggleViewFavorite();
  return (
    <>
      <p data-testid="pending">{toggle.pendingViewId ?? "none"}</p>
      <button type="button" onClick={() => toggle.mutate({ workspaceSlug: "acme", projectId: PROJECT_ID, view: row })}>
        toggle star
      </button>
    </>
  );
}

function renderProbe(row: SavedView, seed = true): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seed) queryClient.setQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT_ID), [row]);
  render(
    <QueryClientProvider client={queryClient}>
      <Probe row={row} />
    </QueryClientProvider>
  );
  return queryClient;
}

function starred(queryClient: QueryClient): boolean | null | undefined {
  return queryClient.getQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT_ID))?.[0]?.is_favorite;
}

beforeEach(() => {
  globalThis.localStorage.clear();
  resetViewFavoriteIds();
});

afterEach(() => {
  resetAppClient();
  resetViewFavoriteIds();
});

describe("selectPendingViewId", () => {
  const input: ToggleViewFavoriteInput = { workspaceSlug: "acme", projectId: PROJECT_ID, view: makeView() };

  it("pins the toggling row while pending", () => {
    expect(selectPendingViewId(true, input)).toBe(input.view.id);
  });

  it("reads null when idle or without variables", () => {
    expect(selectPendingViewId(false, input)).toBeNull();
    expect(selectPendingViewId(true, undefined)).toBeNull();
    expect(selectPendingViewId(false, undefined)).toBeNull();
  });
});

describe("useToggleViewFavorite", () => {
  it("stars with an optimistic flip, then opens the favorites section on first add", async () => {
    const calls = stubClient();
    const queryClient = renderProbe(makeView({ is_favorite: false }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorites/"))).toBe(true)
    );
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
  });

  it("leaves an already-open favorites section alone", async () => {
    globalThis.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const calls = stubClient();
    renderProbe(makeView({ is_favorite: false }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorites/"))).toBe(true)
    );
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
  });

  it("re-reads the sidebar favorites list so the first add surfaces", async () => {
    const calls: string[] = [];
    stubClient({ calls });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const row = makeView({ is_favorite: false });
    queryClient.setQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT_ID), [row]);
    function SectionProbe(): React.ReactElement {
      const list = useFavorites("acme");
      return <p data-testid="fav-count">{(list.data ?? []).length}</p>;
    }
    render(
      <QueryClientProvider client={queryClient}>
        <Probe row={row} />
        <SectionProbe />
      </QueryClientProvider>
    );
    const reads = (): number =>
      calls.filter((call) => call.startsWith("GET") && call.includes("/user-favorites/")).length;
    await waitFor(() => expect(screen.getByTestId("fav-count")).toHaveTextContent("0"));
    expect(reads()).toBe(1);
    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    await waitFor(() => expect(reads()).toBe(2));
  });

  it("unstars through the learned favorite id without a lookup", async () => {
    const calls = stubClient();
    const queryClient = renderProbe(makeView({ is_favorite: false }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    const view = makeView({ is_favorite: true });
    queryClient.setQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT_ID), [view]);
    render(
      <QueryClientProvider client={queryClient}>
        <Probe row={view} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getAllByRole("button", { name: "toggle star" })[1]!);
    await waitFor(() => expect(starred(queryClient)).toBe(false));
    expect(calls.some((call) => call.startsWith("DELETE") && call.includes(`/user-favorites/${FAVORITE_ID}/`))).toBe(
      true
    );
    expect(calls.some((call) => call.startsWith("GET") && call.includes("/user-favorites/"))).toBe(false);
  });

  it("unstars through the favorites list when no id was learned", async () => {
    const calls = stubClient({ favorites: [favoriteRow()] });
    const queryClient = renderProbe(makeView({ is_favorite: true }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(false));
    expect(calls.some((call) => call.startsWith("DELETE") && call.includes(`/user-favorites/${FAVORITE_ID}/`))).toBe(
      true
    );
  });

  it("rolls the star back silently when the write fails", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubClient({ writeStatus: 400, gate });
    const queryClient = renderProbe(makeView({ is_favorite: false }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    release();
    await waitFor(() => expect(starred(queryClient)).toBe(false));
  });

  it("rolls the unstar back when no favorite row exists", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const calls = stubClient({ favorites: [], gate });
    const queryClient = renderProbe(makeView({ is_favorite: true }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(false));
    release();
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    expect(calls.some((call) => call.startsWith("DELETE"))).toBe(false);
  });

  it("toggles without a seeded collection", async () => {
    const calls = stubClient();
    renderProbe(makeView({ is_favorite: false }), false);

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorites/"))).toBe(true)
    );
  });

  it("pins the pending marker on the toggling row", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubClient({ gate });
    renderProbe(makeView({ is_favorite: false }));

    expect(screen.getByTestId("pending")).toHaveTextContent("none");
    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(screen.getByTestId("pending")).toHaveTextContent(VIEW_ID));
    release();
    await waitFor(() => expect(screen.getByTestId("pending")).toHaveTextContent("none"));
  });

  it("leaves the cache alone when the optimistic patch cannot start", async () => {
    stubClient();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const row = makeView({ is_favorite: false });
    queryClient.setQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT_ID), [row]);
    vi.spyOn(queryClient, "cancelQueries").mockRejectedValueOnce(new Error("cache locked"));
    render(
      <QueryClientProvider client={queryClient}>
        <Probe row={row} />
      </QueryClientProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(vi.mocked(queryClient.cancelQueries)).toHaveBeenCalled());
    await new Promise((done) => setTimeout(done, 0));
    expect(starred(queryClient)).toBe(false);
  });
});
