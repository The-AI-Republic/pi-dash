// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page writes (parity/specs/pages.md §2.4, §3.6, §3.8, §7.2, §13.1;
// PAGE-007–012 and the shared PAGE-056 favourite helper): create posts the
// input and re-reads the collection, duplicate confirms by notice, and the
// favourite write flips both caches optimistically, rolls back on failure,
// opens the shell's favourites section on the first add and never races a
// second write.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({
  createPage: vi.fn(),
  duplicatePage: vi.fn(),
  addPageFavorite: vi.fn(),
  removePageFavorite: vi.fn(),
}));

const shell = vi.hoisted(() => ({
  notifyPending: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
  isFavoritesSectionOpen: vi.fn(),
  setFavoritesSectionOpen: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  ...contracts,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyPending: shell.notifyPending,
  notifySuccess: shell.notifySuccess,
  notifyError: shell.notifyError,
}));

vi.mock("../../../shared/shell/favorites.js", () => ({
  isFavoritesSectionOpen: shell.isFavoritesSectionOpen,
  setFavoritesSectionOpen: shell.setFavoritesSectionOpen,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { pageKeys } from "./keys.js";
import {
  flipPageFavorite,
  flipPageFavoriteInList,
  PAGE_CREATE_FAILED_TITLE,
  PAGE_FAVORITE_ADDED_TITLE,
  PAGE_FAVORITE_ADDING_TITLE,
  PAGE_FAVORITE_FAILED_TITLE,
  PAGE_FAVORITE_REMOVED_TITLE,
  PAGE_FAVORITE_REMOVING_TITLE,
  useCreatePage,
  useDuplicatePage,
  usePageFavorite,
  useQuickCreatePage,
} from "./mutations.js";
import { PAGE_DUPLICATE_FAILED_TITLE, PAGE_DUPLICATED_TITLE } from "../lib/pageActions.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";

const SLUG = "acme";
const COPY_ID = "923e4567-e89b-12d3-a456-426614174009";

function newClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

/** Let the mutation's queued microtasks and timers run before asserting. */
async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

interface Pending {
  settle: ReturnType<typeof vi.fn>;
}

let pending: Pending;

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  pending = { settle: vi.fn() };
  shell.notifyPending.mockReturnValue(pending);
  shell.isFavoritesSectionOpen.mockReturnValue(false);
});

afterEach(() => {
  resetAppClient();
});

describe("cached star flips", () => {
  it("leaves an absent row alone", () => {
    expect(flipPageFavorite(undefined)).toBeUndefined();
    expect(flipPageFavoriteInList(undefined, PAGES_PAGE_ID)).toBeUndefined();
  });

  it("flips the matching row and keeps the others as they were", () => {
    const row = makePageRow();
    expect(flipPageFavorite(row)).toEqual({ ...row, is_favorite: true });
    const other = makePageRow({ id: "823e4567-e89b-12d3-a456-426614174007", name: "Other", is_favorite: true });
    expect(flipPageFavoriteInList([row, other], row.id)).toEqual([{ ...row, is_favorite: true }, other]);
  });
});

describe("useCreatePage", () => {
  it("posts the input, re-reads the collection and hands back the new page", async () => {
    const client = newClient();
    const created = { ...makePageRow(), description_html: "", id: COPY_ID, name: "Fresh page" };
    contracts.createPage.mockResolvedValue(created);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => useCreatePage(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });

    let answer: unknown;
    await act(async () => {
      answer = await hook.result.current.mutateAsync({ access: PAGE_ACCESS_PUBLIC, input: { name: "Fresh page" } });
    });

    expect(contracts.createPage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, {
      name: "Fresh page",
    });
    expect(answer).toEqual(created);
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.list(SLUG, PAGES_PROJECT_ID) }));
    expect(shell.notifyError).not.toHaveBeenCalled();
  });

  it("reports a failure and still re-reads the collection", async () => {
    const client = newClient();
    contracts.createPage.mockRejectedValue(new Error("nope"));
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => useCreatePage(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });

    await act(async () => {
      await hook.result.current
        .mutateAsync({ access: PAGE_ACCESS_PUBLIC, input: { name: "Nope" } })
        .catch(() => undefined);
    });

    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_CREATE_FAILED_TITLE);
    expect(PAGE_CREATE_FAILED_TITLE).toMatch(/could not be created/i);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.list(SLUG, PAGES_PROJECT_ID) });
  });
});

describe("useQuickCreatePage", () => {
  it("creates an untitled page in the tab's visibility and reports progress", async () => {
    contracts.createPage.mockResolvedValue(makePageRow({ id: COPY_ID }));
    const client = newClient();
    const hook = renderHook(() => useQuickCreatePage(SLUG, PAGES_PROJECT_ID, PAGE_ACCESS_PRIVATE), {
      wrapper: wrapper(client),
    });
    expect(hook.result.current.pending).toBe(false);

    const onCreated = vi.fn();
    act(() => {
      hook.result.current.create(onCreated);
    });
    await settle();

    expect(contracts.createPage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, {
      name: "Untitled",
      access: PAGE_ACCESS_PRIVATE,
    });
    expect(onCreated).toHaveBeenCalledTimes(1);
    expect((onCreated.mock.calls[0] as [{ id: string }])[0].id).toBe(COPY_ID);
  });

  it("does not hand back a page when the write fails", async () => {
    contracts.createPage.mockRejectedValue(new Error("nope"));
    const client = newClient();
    const hook = renderHook(() => useQuickCreatePage(SLUG, PAGES_PROJECT_ID, PAGE_ACCESS_PUBLIC), {
      wrapper: wrapper(client),
    });
    const onCreated = vi.fn();
    act(() => {
      hook.result.current.create(onCreated);
    });
    await settle();
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_CREATE_FAILED_TITLE);
    expect(onCreated).not.toHaveBeenCalled();
  });
});

describe("useDuplicatePage", () => {
  it("confirms the copy and re-reads the collection", async () => {
    const client = newClient();
    const copy = { ...makePageRow(), id: COPY_ID, name: "Launch notes (Copy)" };
    contracts.duplicatePage.mockResolvedValue(copy);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => useDuplicatePage(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });

    await act(async () => {
      await hook.result.current.mutateAsync({ pageId: PAGES_PAGE_ID });
    });

    expect(contracts.duplicatePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_DUPLICATED_TITLE);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.list(SLUG, PAGES_PROJECT_ID) });
    expect(shell.notifyError).not.toHaveBeenCalled();
  });

  it("reports a failed copy", async () => {
    contracts.duplicatePage.mockRejectedValue(new Error("nope"));
    const client = newClient();
    const hook = renderHook(() => useDuplicatePage(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });

    await act(async () => {
      await hook.result.current.mutateAsync({ pageId: PAGES_PAGE_ID }).catch(() => undefined);
    });

    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_DUPLICATE_FAILED_TITLE);
    expect(shell.notifySuccess).not.toHaveBeenCalled();
  });
});

describe("usePageFavorite", () => {
  function seed(client: QueryClient, starred = false): void {
    client.setQueryData(pageKeys.list(SLUG, PAGES_PROJECT_ID), [makePageRow({ is_favorite: starred })]);
    client.setQueryData(pageKeys.detail(SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID), makePageRow({ is_favorite: starred }));
  }

  function cachedStar(client: QueryClient): { list: boolean; detail: boolean } {
    const list = client.getQueryData<ReturnType<typeof makePageRow>[]>(pageKeys.list(SLUG, PAGES_PROJECT_ID)) ?? [];
    const detail = client.getQueryData<ReturnType<typeof makePageRow>>(
      pageKeys.detail(SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID)
    );
    return { list: list[0]?.is_favorite === true, detail: detail?.is_favorite === true };
  }

  it("adds the star, confirms it, opens the favourites section and re-reads", async () => {
    const client = newClient();
    seed(client);
    contracts.addPageFavorite.mockResolvedValue(undefined);
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(
      () => usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() }),
      { wrapper: wrapper(client) }
    );
    expect(hook.result.current.starred).toBe(false);
    expect(hook.result.current.pending).toBe(false);

    await act(async () => {
      hook.result.current.toggle();
      await waitFor(() => expect(pending.settle).toHaveBeenCalled());
    });

    expect(contracts.addPageFavorite).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(shell.notifyPending).toHaveBeenCalledWith(PAGE_FAVORITE_ADDING_TITLE);
    expect(pending.settle).toHaveBeenCalledWith({
      title: PAGE_FAVORITE_ADDED_TITLE,
      description: "Launch notes is now a favorite.",
    });
    expect(shell.setFavoritesSectionOpen).toHaveBeenCalledWith(true);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.list(SLUG, PAGES_PROJECT_ID) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.detail(SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: expect.arrayContaining(["favorites"]) });
  });

  it("keeps the section open when it already is", async () => {
    shell.isFavoritesSectionOpen.mockReturnValue(true);
    const client = newClient();
    contracts.addPageFavorite.mockResolvedValue(undefined);
    const hook = renderHook(
      () => usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() }),
      { wrapper: wrapper(client) }
    );
    await act(async () => {
      hook.result.current.toggle();
      await waitFor(() => expect(pending.settle).toHaveBeenCalled());
    });
    expect(shell.setFavoritesSectionOpen).not.toHaveBeenCalled();
  });

  it("shows the optimistic star while the add flies and keeps it once it lands", async () => {
    const client = newClient();
    seed(client);
    let release: (() => void) | undefined;
    contracts.addPageFavorite.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const hook = renderHook(
      () => usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() }),
      { wrapper: wrapper(client) }
    );

    act(() => {
      hook.result.current.toggle();
    });
    await waitFor(() => expect(hook.result.current.pending).toBe(true));
    expect(cachedStar(client)).toEqual({ list: true, detail: true });

    await act(async () => {
      release?.();
      await waitFor(() => expect(hook.result.current.pending).toBe(false));
    });
    expect(cachedStar(client)).toEqual({ list: true, detail: true });
  });

  it("removes an existing star without touching the section", async () => {
    const client = newClient();
    seed(client, true);
    contracts.removePageFavorite.mockResolvedValue(undefined);
    const hook = renderHook(
      () =>
        usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow({ is_favorite: true }) }),
      { wrapper: wrapper(client) }
    );
    expect(hook.result.current.starred).toBe(true);

    await act(async () => {
      hook.result.current.toggle();
      await waitFor(() => expect(pending.settle).toHaveBeenCalled());
    });

    expect(contracts.removePageFavorite).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(shell.notifyPending).toHaveBeenCalledWith(PAGE_FAVORITE_REMOVING_TITLE);
    expect(pending.settle).toHaveBeenCalledWith({
      title: PAGE_FAVORITE_REMOVED_TITLE,
      description: "Launch notes is no longer a favorite.",
    });
    expect(shell.setFavoritesSectionOpen).not.toHaveBeenCalled();
  });

  it("rolls the star back and reports the failure", async () => {
    const client = newClient();
    seed(client);
    contracts.addPageFavorite.mockRejectedValue(new Error("Server said no"));
    const hook = renderHook(
      () => usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() }),
      { wrapper: wrapper(client) }
    );

    await act(async () => {
      hook.result.current.toggle();
      await waitFor(() => expect(pending.settle).toHaveBeenCalledWith(expect.objectContaining({ failed: true })));
    });

    expect(pending.settle).toHaveBeenCalledWith({
      title: PAGE_FAVORITE_FAILED_TITLE,
      description: "Server said no",
      failed: true,
    });
    expect(cachedStar(client)).toEqual({ list: false, detail: false });
    expect(shell.setFavoritesSectionOpen).not.toHaveBeenCalled();
  });

  it("reports a failure that is not an Error without a description", async () => {
    const client = newClient();
    seed(client);
    contracts.addPageFavorite.mockRejectedValue("plain string");
    const hook = renderHook(
      () => usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() }),
      { wrapper: wrapper(client) }
    );

    await act(async () => {
      hook.result.current.toggle();
      await waitFor(() => expect(pending.settle).toHaveBeenCalledWith(expect.objectContaining({ failed: true })));
    });
    expect(pending.settle).toHaveBeenCalledWith({ title: PAGE_FAVORITE_FAILED_TITLE, failed: true });
  });

  it("ignores a second press while the first write is in flight", async () => {
    const client = newClient();
    seed(client);
    contracts.addPageFavorite.mockImplementation(() => new Promise<void>(() => undefined));
    const hook = renderHook(
      () => usePageFavorite({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() }),
      { wrapper: wrapper(client) }
    );

    act(() => {
      hook.result.current.toggle();
    });
    await waitFor(() => expect(hook.result.current.pending).toBe(true));
    act(() => {
      hook.result.current.toggle();
    });
    expect(contracts.addPageFavorite).toHaveBeenCalledTimes(1);
  });
});
