// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle favorite toggle (parity/specs/cycles.md §11, CYC-029): the
// workspace-favorites write path, optimistic flips with rollback,
// progress-then-outcome notices, idempotent resolve-before-write, and
// the first-add sidebar reveal.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CycleRow, Favorite } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ pending: vi.fn(), settle: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: (...args: unknown[]) => {
    notify.pending(...args);
    return { id: "notice-1", settle: notify.settle };
  },
}));
const sectionOpen = vi.hoisted(() => ({ open: false, setOpen: vi.fn() }));
vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: () => sectionOpen.open,
  setFavoritesSectionOpen: (open: boolean) => {
    sectionOpen.setOpen(open);
    sectionOpen.open = open;
  },
}));

import * as apiClient from "@pidash/api-client";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { makeCycleRow, CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import {
  CYCLE_FAVORITE_ADDED_TITLE,
  CYCLE_FAVORITE_ADDING_TITLE,
  CYCLE_FAVORITE_FAILED_TITLE,
  CYCLE_FAVORITE_REMOVED_TITLE,
  CYCLE_FAVORITE_REMOVING_TITLE,
  flipCycleFavorite,
  flipCycleFavoriteInList,
  useCycleFavorite,
} from "./cycleFavorite.js";
import { cycleDetailKeys } from "./cycleDetail.js";
import { cycleKeys } from "./keys.js";

const WS = "acme";
const FAVORITE_ID = "423e4567-e89b-12d3-a456-426614174003";

function makeFavorite(overrides: Partial<Favorite> = {}): Favorite {
  return {
    id: FAVORITE_ID,
    entity_type: "cycle",
    entity_identifier: CYCLES_CYCLE_ID,
    entity_data: { name: "Cycle 7" },
    name: null,
    is_folder: false,
    sequence: 1,
    parent: null,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: CYCLES_PROJECT_ID,
    ...overrides,
  };
}

interface Harness {
  favorites: Favorite[];
  posts: unknown[];
  deletes: string[];
  failWrites: boolean;
  listReads: number;
  holdWrites: boolean;
  gate: (() => void) | null;
}

let harness: Harness;
let queryClient: QueryClient;

function respond(url: string, status: number, body: unknown): unknown {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
}

function transport(url: string, init: { method?: string; body?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  const hold = async (): Promise<void> => {
    if (harness.holdWrites) await new Promise<void>((release) => (harness.gate = release));
  };
  if (url.endsWith("/user-favorites/") && method === "GET") {
    harness.listReads += 1;
    return Promise.resolve(respond(url, 200, harness.favorites));
  }
  if (url.endsWith("/user-favorites/") && method === "POST") {
    harness.posts.push(init.body === undefined ? null : JSON.parse(init.body));
    return hold().then(() => {
      if (harness.failWrites) return respond(url, 500, { detail: "boom" });
      const created = makeFavorite({ entity_data: { name: "Cycle 7" } });
      harness.favorites = [created];
      return respond(url, 201, created);
    });
  }
  if (url.includes("/user-favorites/") && method === "DELETE") {
    harness.deletes.push(url);
    return hold().then(() => {
      if (harness.failWrites) return respond(url, 500, { detail: "boom" });
      harness.favorites = [];
      return respond(url, 204, null);
    });
  }
  return Promise.resolve(respond(url, 200, []));
}

function Host({ cycle }: { cycle: Pick<CycleRow, "id" | "name" | "is_favorite"> }): React.ReactElement {
  const { starred, pending, toggle } = useCycleFavorite({ workspaceSlug: WS, projectId: CYCLES_PROJECT_ID, cycle });
  return (
    <button type="button" data-testid="star" data-starred={starred} data-pending={pending} onClick={toggle}>
      star
    </button>
  );
}

function renderHost(cycle: Pick<CycleRow, "id" | "name" | "is_favorite">): { unmount: () => void } {
  return render(
    <QueryClientProvider client={queryClient}>
      <Host cycle={cycle} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  harness = { favorites: [], posts: [], deletes: [], failWrites: false, listReads: 0, holdWrites: false, gate: null };
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  createAppClient("https://api.test", transport as never);
  sectionOpen.open = false;
  notify.pending.mockClear();
  notify.settle.mockClear();
  sectionOpen.setOpen.mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("flipCycleFavorite", () => {
  it("flips the mark and keeps undefined undefined", () => {
    expect(flipCycleFavorite({ id: "a", is_favorite: false })).toEqual({ id: "a", is_favorite: true });
    expect(flipCycleFavorite({ id: "a", is_favorite: true })).toEqual({ id: "a", is_favorite: false });
    expect(flipCycleFavorite(undefined)).toBeUndefined();
  });
});

describe("flipCycleFavoriteInList", () => {
  it("flips only the matching row and keeps undefined undefined", () => {
    const rows = [
      { id: "a", is_favorite: false },
      { id: "b", is_favorite: false },
    ];
    expect(flipCycleFavoriteInList(rows, "a")).toEqual([
      { id: "a", is_favorite: true },
      { id: "b", is_favorite: false },
    ]);
    expect(flipCycleFavoriteInList(undefined, "a")).toBeUndefined();
  });
});

describe("useCycleFavorite", () => {
  it("adds through the workspace favorites tree with the cycle name", async () => {
    const row = makeCycleRow({ is_favorite: false });
    queryClient.setQueryData(cycleKeys.list(WS, CYCLES_PROJECT_ID), [row]);
    renderHost(row);
    await act(async () => {
      screen.getByTestId("star").click();
    });
    expect(harness.posts).toHaveLength(1);
    expect(harness.posts[0]).toMatchObject({
      entity_type: "cycle",
      entity_identifier: CYCLES_CYCLE_ID,
      project_id: CYCLES_PROJECT_ID,
      entity_data: { name: row.name },
    });
    expect(notify.pending).toHaveBeenCalledWith(CYCLE_FAVORITE_ADDING_TITLE);
    expect(notify.settle).toHaveBeenCalledWith(
      expect.objectContaining({
        title: CYCLE_FAVORITE_ADDED_TITLE,
        description: expect.stringMatching(/added|favorite/i),
      })
    );
    expect(sectionOpen.setOpen).toHaveBeenCalledWith(true);
  });

  it("outcome copy matches the oracle's added/removed reads", async () => {
    const row = makeCycleRow({ is_favorite: false });
    const first = renderHost(row);
    await act(async () => {
      screen.getByTestId("star").click();
    });
    const outcome = notify.settle.mock.calls[0]?.[0] as { title: string; description?: string } | undefined;
    expect(`${outcome?.title} ${outcome?.description}`).toMatch(/added/i);
    first.unmount();

    notify.settle.mockClear();
    harness.favorites = [makeFavorite()];
    const second = renderHost({ ...row, is_favorite: true });
    await act(async () => {
      screen.getByTestId("star").click();
    });
    const removed = notify.settle.mock.calls[0]?.[0] as { title: string; description?: string } | undefined;
    expect(`${removed?.title} ${removed?.description}`).toMatch(/removed/i);
    expect(notify.pending).toHaveBeenCalledWith(CYCLE_FAVORITE_REMOVING_TITLE);
    expect(removed?.title).toBe(CYCLE_FAVORITE_REMOVED_TITLE);
    second.unmount();
  });

  it("removes by resolving the favorite id first", async () => {
    harness.favorites = [makeFavorite()];
    const row = makeCycleRow({ is_favorite: true });
    queryClient.setQueryData(cycleDetailKeys.detail(WS, CYCLES_PROJECT_ID, CYCLES_CYCLE_ID), row);
    renderHost(row);
    await act(async () => {
      screen.getByTestId("star").click();
    });
    expect(harness.deletes).toHaveLength(1);
    expect(harness.deletes[0]).toContain(FAVORITE_ID);
  });

  it("shows the optimistic mark while the write flies and settles on success", async () => {
    harness.holdWrites = true;
    const row = makeCycleRow({ is_favorite: false });
    renderHost(row);
    const star = screen.getByTestId("star");
    expect(star).toHaveAttribute("data-starred", "false");
    fireEvent.click(star);
    await waitFor(() => expect(screen.getByTestId("star")).toHaveAttribute("data-starred", "true"));
    expect(screen.getByTestId("star")).toHaveAttribute("data-pending", "true");
    await act(async () => {
      harness.gate?.();
    });
    await waitFor(() => expect(screen.getByTestId("star")).toHaveAttribute("data-pending", "false"));
  });

  it("refuses a second toggle while the first write flies", async () => {
    harness.holdWrites = true;
    renderHost(makeCycleRow({ is_favorite: false }));
    fireEvent.click(screen.getByTestId("star"));
    await waitFor(() => expect(screen.getByTestId("star")).toHaveAttribute("data-pending", "true"));
    fireEvent.click(screen.getByTestId("star"));
    await act(async () => {
      harness.gate?.();
    });
    await waitFor(() => expect(harness.posts).toHaveLength(1));
  });

  it("rolls the star back and reports the failure", async () => {
    harness.failWrites = true;
    const row = makeCycleRow({ is_favorite: false });
    queryClient.setQueryData(cycleKeys.list(WS, CYCLES_PROJECT_ID), [row]);
    queryClient.setQueryData(cycleKeys.hero(WS, CYCLES_PROJECT_ID), [row]);
    queryClient.setQueryData(cycleDetailKeys.detail(WS, CYCLES_PROJECT_ID, CYCLES_CYCLE_ID), row);
    renderHost(row);
    await act(async () => {
      screen.getByTestId("star").click();
    });
    expect(notify.settle).toHaveBeenCalledWith(expect.objectContaining({ title: CYCLE_FAVORITE_FAILED_TITLE }));
    const settled = notify.settle.mock.calls[0]?.[0] as { failed?: boolean };
    expect(settled.failed).toBe(true);
    expect(queryClient.getQueryData(cycleKeys.list(WS, CYCLES_PROJECT_ID))).toEqual([row]);
    expect(queryClient.getQueryData(cycleKeys.hero(WS, CYCLES_PROJECT_ID))).toEqual([row]);
    expect(queryClient.getQueryData(cycleDetailKeys.detail(WS, CYCLES_PROJECT_ID, CYCLES_CYCLE_ID))).toEqual(row);
  });

  it("settles title-only when the failure carries no message", async () => {
    const reads = vi.spyOn(apiClient, "listFavorites").mockRejectedValue("raw failure");
    try {
      renderHost(makeCycleRow({ is_favorite: false }));
      await act(async () => {
        screen.getByTestId("star").click();
      });
      expect(notify.settle).toHaveBeenCalledWith(
        expect.objectContaining({ title: CYCLE_FAVORITE_FAILED_TITLE, failed: true })
      );
      const settled = notify.settle.mock.calls[0]?.[0] as { description?: string };
      expect(settled.description).toBeUndefined();
    } finally {
      reads.mockRestore();
    }
  });

  it("skips the POST when the entry already exists and skips the DELETE when it does not", async () => {
    harness.favorites = [makeFavorite()];
    const first = renderHost(makeCycleRow({ is_favorite: false }));
    await act(async () => {
      screen.getByTestId("star").click();
    });
    expect(harness.posts).toHaveLength(0);
    expect(notify.settle).toHaveBeenCalledWith(expect.objectContaining({ title: CYCLE_FAVORITE_ADDED_TITLE }));
    first.unmount();

    harness.favorites = [];
    const second = renderHost(makeCycleRow({ is_favorite: true }));
    await act(async () => {
      screen.getByTestId("star").click();
    });
    expect(harness.deletes).toHaveLength(0);
    expect(notify.settle).toHaveBeenCalledWith(expect.objectContaining({ title: CYCLE_FAVORITE_REMOVED_TITLE }));
    second.unmount();
  });

  it("re-reads the cycle and the sidebar list after the toggle settles", async () => {
    const row = makeCycleRow({ is_favorite: false });
    queryClient.setQueryData(cycleKeys.list(WS, CYCLES_PROJECT_ID), [row]);
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    renderHost(row);
    await act(async () => {
      screen.getByTestId("star").click();
    });
    const keys = invalidate.mock.calls.map((call) => JSON.stringify(call[0]?.queryKey ?? null));
    expect(keys.some((key) => key.includes('"list"'))).toBe(true);
    expect(keys.some((key) => key.includes('"hero"'))).toBe(true);
    expect(keys).toContain(JSON.stringify(sidebarKeys.favorites(WS)));
    invalidate.mockRestore();
  });

  it("leaves an already-open favorites section alone on later adds", async () => {
    sectionOpen.open = true;
    renderHost(makeCycleRow({ is_favorite: false }));
    await act(async () => {
      screen.getByTestId("star").click();
    });
    expect(sectionOpen.setOpen).not.toHaveBeenCalled();
  });
});
