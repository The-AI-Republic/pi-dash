// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module favorite toggle (parity/specs/modules.md §4.4, MOD-016): the
// star flips at once through the user-favorite-modules endpoints, both
// directions notice their pinned outcome, a failure rolls the star back,
// and the first add opens the sidebar favorites section. A second toggle
// while a write flies waits instead of double-driving.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  favoriteModule: vi.fn(),
  unfavoriteModule: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  favoriteModule: mocks.favoriteModule,
  unfavoriteModule: mocks.unfavoriteModule,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { FAVORITES_SECTION_OPEN_KEY } from "../../../shared/shell/favorites.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { moduleKeys } from "../api/keys.js";
import { makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../testing.js";
import {
  flipModuleFavorite,
  flipModuleFavoriteInList,
  MODULE_FAVORITE_ADDED,
  MODULE_FAVORITE_REMOVED,
  useModuleFavorite,
} from "./useModuleFavorite.js";

const SLUG = "acme";

function mount(row: ModuleRow) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(moduleKeys.list(SLUG, MODULES_PROJECT_ID), [row]);
  const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue(undefined);
  const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const view = renderHook(
    ({ favorite }: { favorite: boolean }) =>
      useModuleFavorite({
        workspaceSlug: SLUG,
        projectId: MODULES_PROJECT_ID,
        module: { id: row.id, name: row.name, is_favorite: favorite },
      }),
    { wrapper, initialProps: { favorite: row.is_favorite } }
  );
  return { ...view, queryClient, invalidate };
}

function cachedFavorite(queryClient: QueryClient): boolean {
  const rows = queryClient.getQueryData<ModuleRow[]>(moduleKeys.list(SLUG, MODULES_PROJECT_ID));
  if (rows === undefined || rows[0] === undefined) throw new Error("expected a cached module row");
  return rows[0].is_favorite;
}

/** Hold the favorite write until the test proves the optimistic flip, then settle it. */
function gateWrite(mock: { mockImplementationOnce: (fn: () => Promise<void>) => void }): {
  release: () => void;
  fail: (error: unknown) => void;
} {
  let release!: () => void;
  let fail!: (error: unknown) => void;
  mock.mockImplementationOnce(
    () =>
      new Promise<void>((resolve, reject) => {
        release = resolve;
        fail = reject;
      })
  );
  return {
    release: () => release(),
    fail: (error: unknown) => fail(error),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  mocks.favoriteModule.mockResolvedValue(undefined);
  mocks.unfavoriteModule.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("flipModuleFavorite", () => {
  it("flips one row and leaves an absent row absent", () => {
    expect(flipModuleFavorite(undefined)).toBeUndefined();
    expect(flipModuleFavorite({ id: "m", is_favorite: false })).toEqual({ id: "m", is_favorite: true });
    expect(flipModuleFavorite({ id: "m", is_favorite: true })).toEqual({ id: "m", is_favorite: false });
  });

  it("flips only the matching row inside a list", () => {
    expect(flipModuleFavoriteInList(undefined, "m")).toBeUndefined();
    const rows = [
      { id: "m", is_favorite: false },
      { id: "other", is_favorite: false },
    ];
    expect(flipModuleFavoriteInList(rows, "m")).toEqual([
      { id: "m", is_favorite: true },
      { id: "other", is_favorite: false },
    ]);
  });
});

describe("useModuleFavorite", () => {
  it("favorites with the pinned notice and opens the sidebar section", async () => {
    const { result, queryClient, invalidate } = mount(makeModuleRow({ is_favorite: false }));
    expect(result.current.starred).toBe(false);
    expect(result.current.pending).toBe(false);
    const write = gateWrite(mocks.favoriteModule);

    act(() => {
      result.current.toggle();
    });
    await waitFor(() => {
      expect(cachedFavorite(queryClient)).toBe(true);
    });

    await act(async () => {
      write.release();
    });
    await waitFor(() => {
      expect(mocks.notifySuccess).toHaveBeenCalledTimes(1);
    });
    expect(mocks.favoriteModule).toHaveBeenCalledWith(expect.anything(), SLUG, MODULES_PROJECT_ID, MODULES_MODULE_ID);
    expect(mocks.unfavoriteModule).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", MODULE_FAVORITE_ADDED);
    expect(mocks.notifyError).not.toHaveBeenCalled();
    expect(window.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
    expect(cachedFavorite(queryClient)).toBe(true);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: sidebarKeys.favorites(SLUG) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: moduleKeys.list(SLUG, MODULES_PROJECT_ID) });
  });

  it("unfavorites with the pinned notice and leaves the section flag alone", async () => {
    const { result, queryClient } = mount(makeModuleRow({ is_favorite: true }));
    const write = gateWrite(mocks.unfavoriteModule);

    act(() => {
      result.current.toggle();
    });
    await waitFor(() => {
      expect(cachedFavorite(queryClient)).toBe(false);
    });

    await act(async () => {
      write.release();
    });
    await waitFor(() => {
      expect(mocks.notifySuccess).toHaveBeenCalledTimes(1);
    });
    expect(mocks.unfavoriteModule).toHaveBeenCalledWith(expect.anything(), SLUG, MODULES_PROJECT_ID, MODULES_MODULE_ID);
    expect(mocks.favoriteModule).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", MODULE_FAVORITE_REMOVED);
    expect(window.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBeNull();
    expect(cachedFavorite(queryClient)).toBe(false);
  });

  it("leaves an already-open section open", async () => {
    window.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    const { result } = mount(makeModuleRow({ is_favorite: false }));

    act(() => {
      result.current.toggle();
    });

    await waitFor(() => {
      expect(mocks.favoriteModule).toHaveBeenCalledTimes(1);
    });
    expect(window.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
  });

  it("rolls the star back with an error notice when the write fails", async () => {
    const { result, queryClient } = mount(makeModuleRow({ is_favorite: false }));
    const write = gateWrite(mocks.favoriteModule);

    act(() => {
      result.current.toggle();
    });
    await waitFor(() => {
      expect(cachedFavorite(queryClient)).toBe(true);
    });

    await act(async () => {
      write.fail(new Error("offline"));
    });
    await waitFor(() => {
      expect(mocks.notifyError).toHaveBeenCalledTimes(1);
    });
    expect(mocks.notifyError).toHaveBeenCalledWith("Error!", "offline");
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
    expect(cachedFavorite(queryClient)).toBe(false);
    expect(window.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBeNull();
  });

  it("refuses a second toggle while the first write flies", async () => {
    const write = gateWrite(mocks.favoriteModule);
    const { result, rerender } = mount(makeModuleRow({ is_favorite: false }));

    act(() => {
      result.current.toggle();
    });
    // Mid-flight the mark is the optimistic target with progress pinned.
    await waitFor(() => {
      expect(result.current.pending).toBe(true);
    });
    expect(result.current.starred).toBe(true);

    act(() => {
      result.current.toggle();
    });
    expect(mocks.favoriteModule).toHaveBeenCalledTimes(1);

    await act(async () => {
      write.release();
    });
    await waitFor(() => {
      expect(result.current.pending).toBe(false);
    });
    // Settled, the mark follows the re-read row production hands back.
    rerender({ favorite: true });
    expect(result.current.starred).toBe(true);
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", MODULE_FAVORITE_ADDED);
  });
});
