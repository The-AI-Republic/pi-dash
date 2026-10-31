// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import type { Favorite, FavoriteList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  applyFavoriteParent,
  applyFavoriteSequence,
  compareFavoriteSequence,
  FAVORITE_REMOVED_TITLE,
  favoriteOrderPatch,
  FOLDER_DUPLICATE_TITLE,
  FOLDER_EMPTY_TITLE,
  moveFavoriteToIndex,
  resolveIntoFolder,
  selectOrderedFavorites,
  selectTopLevelFavorites,
  sequenceForInsert,
  sequenceForMove,
  useCreateFavoriteFolder,
  useDeleteFavorite,
  useFavoriteGroup,
  useFavorites,
  useMoveFavorite,
  useRenameFavoriteFolder,
  validateFolderName,
  type FavoriteDropSpan,
  type FavoriteMoveInput,
} from "./favorites.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const FOLDER_ID = "223e4567-e89b-12d3-a456-426614174001";
const ENTRY_ID = "323e4567-e89b-12d3-a456-426614174002";
const CHILD_ID = "423e4567-e89b-12d3-a456-426614174003";
const PROJECT_ID = "523e4567-e89b-12d3-a456-426614174004";

function row(overrides: Partial<Favorite> = {}): Favorite {
  return {
    id: ENTRY_ID,
    entity_type: "project",
    entity_identifier: PROJECT_ID,
    entity_data: {},
    name: "Atlas",
    is_folder: false,
    sequence: 20000,
    parent: null,
    workspace_id: WS_ID,
    project_id: PROJECT_ID,
    ...overrides,
  };
}

const FOLDER = row({
  id: FOLDER_ID,
  entity_type: "folder",
  entity_identifier: null,
  name: "Pinned",
  is_folder: true,
  sequence: 30000,
  project_id: null,
});
const ENTRY = row({ id: ENTRY_ID, sequence: 20000 });
const CHILD = row({ id: CHILD_ID, name: "Nested", sequence: 10000, parent: FOLDER_ID });

describe("favorite ordering selectors", () => {
  it("sorts descending by sequence without mutating", () => {
    const list = [ENTRY, FOLDER];
    expect(selectOrderedFavorites(list).map((fav) => fav.id)).toEqual([FOLDER_ID, ENTRY_ID]);
    expect(list[0]?.id).toBe(ENTRY_ID);
  });

  it("compares sequences high-first", () => {
    expect(compareFavoriteSequence({ sequence: 1 }, { sequence: 2 })).toBeGreaterThan(0);
    expect(compareFavoriteSequence({ sequence: 2 }, { sequence: 1 })).toBeLessThan(0);
    expect(compareFavoriteSequence({ sequence: 2 }, { sequence: 2 })).toBe(0);
  });

  it("keeps top-level rows only, in display order", () => {
    expect(selectTopLevelFavorites([CHILD, ENTRY, FOLDER]).map((fav) => fav.id)).toEqual([FOLDER_ID, ENTRY_ID]);
  });
});

describe("validateFolderName", () => {
  it("accepts a fresh name", () => {
    expect(validateFolderName("Fresh", ["Pinned"])).toBeNull();
  });

  it("rejects blank-after-trimming names", () => {
    expect(validateFolderName("   ", ["Pinned"])).toBe("empty");
  });

  it("rejects trimmed duplicates", () => {
    expect(validateFolderName("Pinned", ["Pinned"])).toBe("duplicate");
    expect(validateFolderName("  Pinned  ", ["Pinned"])).toBe("duplicate");
  });
});

describe("moveFavoriteToIndex", () => {
  it("moves the dragged id to the slot", () => {
    expect(moveFavoriteToIndex(["a", "b", "c"], "c", 0)).toEqual(["c", "a", "b"]);
    expect(moveFavoriteToIndex(["a", "b", "c"], "a", 2)).toEqual(["b", "c", "a"]);
  });

  it("clamps slots and keeps unknown ids put", () => {
    expect(moveFavoriteToIndex(["a", "b"], "b", 99)).toEqual(["a", "b"]);
    expect(moveFavoriteToIndex(["a", "b"], "b", -4)).toEqual(["b", "a"]);
    expect(moveFavoriteToIndex(["a", "b"], "zzz", 0)).toEqual(["a", "b"]);
  });
});

describe("sequenceForMove", () => {
  const ordered = [
    { id: "a", sequence: 30000 },
    { id: "b", sequence: 20000 },
    { id: "c", sequence: 10000 },
  ];

  it("jumps past the edge neighbor at either end", () => {
    expect(sequenceForMove(ordered, "c", ["c", "a", "b"])).toBe(40000);
    expect(sequenceForMove(ordered, "a", ["b", "c", "a"])).toBe(0);
  });

  it("lands midway between the new neighbors in the middle", () => {
    expect(sequenceForMove(ordered, "a", ["b", "a", "c"])).toBe(15000);
  });

  it("rejects unknown ids and length mismatches", () => {
    expect(sequenceForMove(ordered, "zzz", ["a", "b", "c"])).toBeNull();
    expect(sequenceForMove(ordered, "a", ["a", "b"])).toBeNull();
  });
});

describe("favoriteOrderPatch", () => {
  const ordered = [
    { id: "a", sequence: 30000 },
    { id: "b", sequence: 20000 },
  ];

  it("writes the dragged row's new sequence", () => {
    expect(favoriteOrderPatch(ordered, ["b", "a"], "b")).toEqual({ favoriteId: "b", sequence: 40000 });
  });

  it("stays silent when the drop changes nothing", () => {
    expect(favoriteOrderPatch(ordered, ["a", "b"], "b")).toBeNull();
    expect(favoriteOrderPatch(ordered, ["a"], "a")).toBeNull();
    expect(favoriteOrderPatch(ordered, ["b", "a"], "zzz")).toBeNull();
  });
});

describe("applyFavoriteSequence", () => {
  it("applies the sequence to the dragged row only", () => {
    expect(applyFavoriteSequence([FOLDER, ENTRY], ENTRY_ID, 999)).toEqual([FOLDER, { ...ENTRY, sequence: 999 }]);
  });

  it("passes an empty cache through", () => {
    expect(applyFavoriteSequence(undefined, ENTRY_ID, 999)).toBeUndefined();
  });
});

describe("applyFavoriteParent", () => {
  it("applies the parent to the dragged row only", () => {
    expect(applyFavoriteParent([FOLDER, ENTRY], ENTRY_ID, FOLDER_ID)).toEqual([
      FOLDER,
      { ...ENTRY, parent: FOLDER_ID },
    ]);
    expect(applyFavoriteParent([CHILD], CHILD_ID, null)).toEqual([{ ...CHILD, parent: null }]);
  });

  it("passes an empty cache through", () => {
    expect(applyFavoriteParent(undefined, ENTRY_ID, FOLDER_ID)).toBeUndefined();
  });
});

describe("sequenceForInsert", () => {
  const ordered = [
    { id: "a", sequence: 30000 },
    { id: "b", sequence: 20000 },
    { id: "c", sequence: 10000 },
  ];

  it("jumps past the edge neighbor at either end", () => {
    expect(sequenceForInsert(ordered, 0)).toBe(40000);
    expect(sequenceForInsert(ordered, 3)).toBe(0);
  });

  it("lands midway between the new neighbors in the middle", () => {
    expect(sequenceForInsert(ordered, 1)).toBe(25000);
    expect(sequenceForInsert(ordered, 2)).toBe(15000);
  });

  it("clamps slots and starts an empty list a step above zero", () => {
    expect(sequenceForInsert(ordered, 99)).toBe(0);
    expect(sequenceForInsert(ordered, -4)).toBe(40000);
    expect(sequenceForInsert([], 0)).toBe(10000);
  });
});

describe("resolveIntoFolder", () => {
  const folder: FavoriteDropSpan = {
    id: FOLDER_ID,
    top: 0,
    bottom: 100,
    isFolder: true,
    headerTop: 0,
    headerBottom: 40,
    headerLeft: 0,
    headerRight: 200,
  };
  const entry: FavoriteDropSpan = {
    id: ENTRY_ID,
    top: 100,
    bottom: 140,
    isFolder: false,
    headerTop: 100,
    headerBottom: 140,
    headerLeft: 0,
    headerRight: 200,
  };

  it("matches the folder header's middle band", () => {
    expect(resolveIntoFolder([folder, entry], 100, 20, ENTRY_ID)).toBe(FOLDER_ID);
  });

  it("ignores plain entries, the dragged row and header edges", () => {
    expect(resolveIntoFolder([entry], 100, 120, FOLDER_ID)).toBeNull();
    expect(resolveIntoFolder([folder], 100, 20, FOLDER_ID)).toBeNull();
    expect(resolveIntoFolder([folder, entry], 100, 2, ENTRY_ID)).toBeNull();
    expect(resolveIntoFolder([folder, entry], 100, 38, ENTRY_ID)).toBeNull();
    expect(resolveIntoFolder([folder, entry], 5, 20, ENTRY_ID)).toBeNull();
    expect(resolveIntoFolder([folder, entry], 195, 20, ENTRY_ID)).toBeNull();
  });

  it("ignores degenerate headers", () => {
    const flat = { ...folder, headerTop: 20, headerBottom: 20 };
    expect(resolveIntoFolder([flat], 100, 20, ENTRY_ID)).toBeNull();
    const narrow = { ...folder, headerLeft: 100, headerRight: 100 };
    expect(resolveIntoFolder([narrow], 100, 20, ENTRY_ID)).toBeNull();
    expect(resolveIntoFolder([], 100, 20, ENTRY_ID)).toBeNull();
  });
});

interface StubOptions {
  list?: FavoriteList;
  group?: FavoriteList;
  failMethod?: string;
}

function stubClient(options: StubOptions = {}) {
  const server: FavoriteList = (options.list ?? []).map((fav) => ({ ...fav }));
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ url, method, body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (method === options.failMethod) {
      return { status: 500, url, headers: { get: () => null }, text: async () => "{}" };
    }
    if (url.endsWith("/user-favorites/") && method === "GET") {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(server) };
    }
    const groupMatch = /\/user-favorites\/([^/]+)\/group\/$/.exec(url);
    if (groupMatch !== null && method === "GET") {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(options.group ?? []) };
    }
    if (url.endsWith("/user-favorites/") && method === "POST") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      return {
        status: 201,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(row({ id: FOLDER_ID, ...sent })),
      };
    }
    const patchMatch = /\/user-favorites\/([^/]+)\/$/.exec(url);
    if (patchMatch !== null && method === "PATCH") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      const target = server.find((fav) => fav.id === patchMatch[1]);
      if (target !== undefined) Object.assign(target, sent);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(row({ ...sent })),
      };
    }
    if (method === "DELETE") {
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls };
}

function renderHandle(options: StubOptions = {}) {
  const { calls } = stubClient(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let create: ((input: { name: string }) => void) | null = null;
  let rename: ((input: { favoriteId: string; name: string }) => void) | null = null;
  let remove: ((input: { favoriteId: string }) => void) | null = null;
  let move: ((input: FavoriteMoveInput) => void) | null = null;
  function Probe(): React.ReactElement {
    const list = useFavorites("acme");
    const group = useFavoriteGroup("acme", FOLDER_ID);
    const createMutation = useCreateFavoriteFolder("acme");
    const renameMutation = useRenameFavoriteFolder("acme");
    const deleteMutation = useDeleteFavorite("acme");
    const moveMutation = useMoveFavorite("acme");
    create = (input) => createMutation.mutate(input);
    rename = (input) => renameMutation.mutate(input);
    remove = (input) => deleteMutation.mutate(input);
    move = (input) => moveMutation.mutate(input);
    const names = (list.data ?? []).map((fav) => `${fav.name}:${fav.sequence}`).join(",");
    const kids = (group.data ?? []).map((fav) => fav.name).join(",");
    return <p>{`${list.status}|${names}|${group.status}|${kids}`}</p>;
  }
  render(
    <QueryClientProvider client={queryClient}>
      <Probe />
    </QueryClientProvider>
  );
  return {
    calls,
    text: () => screen.getByText(/\|/).textContent ?? "",
    create: (input: { name: string }) => create?.(input),
    rename: (input: { favoriteId: string; name: string }) => rename?.(input),
    remove: (input: { favoriteId: string }) => remove?.(input),
    move: (input: FavoriteMoveInput) => move?.(input),
  };
}

describe("favorite hooks", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("reads the top-level list and one folder's children", async () => {
    const h = renderHandle({ list: [FOLDER, ENTRY], group: [CHILD] });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000,Atlas:20000|success|Nested"));
  });

  it("creates a folder with a trimmed name and a success notice", async () => {
    const h = renderHandle({ list: [] });
    await waitFor(() => expect(h.text()).toBe("success||success|"));
    h.create({ name: "  Fresh  " });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Favorite created successfully"));
    const post = h.calls.find((call) => call.method === "POST");
    expect(post?.url).toContain("/api/workspaces/acme/user-favorites/");
    expect(JSON.parse(post?.body ?? "{}")).toMatchObject({
      entity_type: "folder",
      name: "Fresh",
      is_folder: true,
      parent: null,
      project_id: null,
    });
  });

  it("toasts a failure when the folder create fails", async () => {
    const h = renderHandle({ list: [], failMethod: "POST" });
    await waitFor(() => expect(h.text()).toBe("success||success|"));
    h.create({ name: "Fresh" });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", expect.any(String)));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("renames a folder with a trimmed name and a success notice", async () => {
    const h = renderHandle({ list: [FOLDER] });
    await waitFor(() => expect(h.text()).toContain("Pinned"));
    h.rename({ favoriteId: FOLDER_ID, name: "  Sorted  " });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Favorite updated successfully"));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/user-favorites/${FOLDER_ID}/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ name: "Sorted" });
  });

  it("toasts a failure when the rename fails", async () => {
    const h = renderHandle({ list: [FOLDER], failMethod: "PATCH" });
    await waitFor(() => expect(h.text()).toContain("Pinned"));
    h.rename({ favoriteId: FOLDER_ID, name: "Sorted" });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", expect.any(String)));
  });

  it("removes an entry with the pinned notice", async () => {
    const h = renderHandle({ list: [ENTRY] });
    await waitFor(() => expect(h.text()).toContain("Atlas"));
    h.remove({ favoriteId: ENTRY_ID });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(FAVORITE_REMOVED_TITLE));
    const call = h.calls.find((entry) => entry.method === "DELETE");
    expect(call?.url).toContain(`/api/workspaces/acme/user-favorites/${ENTRY_ID}/`);
  });

  it("toasts a failure when the removal fails", async () => {
    const h = renderHandle({ list: [ENTRY], failMethod: "DELETE" });
    await waitFor(() => expect(h.text()).toContain("Atlas"));
    h.remove({ favoriteId: ENTRY_ID });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", expect.any(String)));
    expect(notifySuccess).not.toHaveBeenCalledWith(FAVORITE_REMOVED_TITLE);
  });

  it("moves a top-level row optimistically and persists the sequence", async () => {
    const h = renderHandle({ list: [FOLDER, ENTRY] });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000,Atlas:20000|success|"));
    h.move({ favoriteId: ENTRY_ID, sequence: 40000, groupId: null });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000,Atlas:40000|success|"));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sequence: 40000 });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("moves a nested row against its folder's cache", async () => {
    const h = renderHandle({ list: [FOLDER], group: [CHILD] });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000|success|Nested"));
    h.move({ favoriteId: CHILD_ID, sequence: 500, groupId: FOLDER_ID });
    await waitFor(() =>
      expect(h.calls.some((call) => call.method === "PATCH" && call.url.includes(CHILD_ID))).toBe(true)
    );
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("nests a row with a parent-only patch", async () => {
    const h = renderHandle({ list: [FOLDER, ENTRY] });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000,Atlas:20000|success|"));
    h.move({ favoriteId: ENTRY_ID, groupId: null, parent: FOLDER_ID });
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ parent: FOLDER_ID });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("un-nests a row with a parent-plus-sequence patch", async () => {
    const h = renderHandle({ list: [FOLDER], group: [CHILD] });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000|success|Nested"));
    h.move({ favoriteId: CHILD_ID, groupId: FOLDER_ID, sequence: 40000, parent: null });
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ sequence: 40000, parent: null });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("sends no patch when a move carries neither field", async () => {
    const h = renderHandle({ list: [FOLDER, ENTRY] });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000,Atlas:20000|success|"));
    h.move({ favoriteId: ENTRY_ID, groupId: null });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(h.calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reverts the optimistic sequence and toasts when the move fails", async () => {
    const h = renderHandle({ list: [FOLDER, ENTRY], failMethod: "PATCH" });
    await waitFor(() => expect(h.text()).toBe("success|Pinned:30000,Atlas:20000|success|"));
    h.move({ favoriteId: ENTRY_ID, sequence: 40000, groupId: null });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", expect.any(String)));
    expect(h.text()).toBe("success|Pinned:30000,Atlas:20000|success|");
  });

  it("pins the folder-form rejection titles", () => {
    expect(FOLDER_EMPTY_TITLE).toBe("Folder name cannot be empty");
    expect(FOLDER_DUPLICATE_TITLE).toBe("Folder already exists");
  });
});
