// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row state writes (parity/specs/pages.md §2.3, §2.4, §3.8, §11.1;
// PAGE-013–017): each write calls its one contract, flips the cached row
// optimistically, rolls back on refusal, notices with the pinned copy and
// publishes the collaboration event on success.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({
  setPageAccess: vi.fn(),
  lockPage: vi.fn(),
  unlockPage: vi.fn(),
  archivePage: vi.fn(),
  restorePage: vi.fn(),
  deletePage: vi.fn(),
  movePageToProject: vi.fn(),
}));

const shell = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  ...contracts,
}));
vi.mock("../../../shared/shell/toaster.js", () => shell);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { createRealtimeHub, getRealtimeHub, resetRealtimeHub } from "../../../core/query/realtime.js";
import { pageKeys } from "./keys.js";
import {
  PAGE_EVENT_ARCHIVED,
  PAGE_EVENT_DELETED,
  PAGE_EVENT_LOCKED,
  PAGE_EVENT_MADE_PRIVATE,
  PAGE_EVENT_MADE_PUBLIC,
  PAGE_EVENT_MOVED,
  PAGE_EVENT_UNARCHIVED,
  PAGE_EVENT_UNLOCKED,
  PAGES_REALTIME_TOPIC,
  patchPageRow,
  publishPageEvent,
  removePageRow,
  usePageArchiveWrite,
  usePageDeleteWrite,
  usePageLockWrite,
  usePageMoveWrite,
  usePageVisibilityWrite,
} from "./rowMutations.js";
import {
  PAGE_ARCHIVED_TITLE,
  PAGE_ARCHIVE_FAILED_TITLE,
  PAGE_DELETED_TITLE,
  PAGE_DELETE_FAILED_TITLE,
  PAGE_LOCKED_TITLE,
  PAGE_LOCK_FAILED_TITLE,
  PAGE_MOVED_TITLE,
  PAGE_MOVE_FAILED_TITLE,
  PAGE_RESTORED_TITLE,
  PAGE_RESTORE_FAILED_TITLE,
  PAGE_UNLOCKED_TITLE,
  PAGE_VISIBILITY_FAILED_TITLE,
} from "../lib/pageActions.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";

const SLUG = "acme";
const OTHER_PROJECT = "723e4567-e89b-12d3-a456-426614174008";
const OTHER_ROW = "823e4567-e89b-12d3-a456-426614174007";

function newClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function target() {
  return { workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, page: makePageRow() };
}

function seed(client: QueryClient, row = makePageRow(), detail = makePageRow()): void {
  client.setQueryData(pageKeys.list(SLUG, PAGES_PROJECT_ID), [row, makePageRow({ id: OTHER_ROW, name: "Other" })]);
  client.setQueryData(pageKeys.detail(SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID), detail);
}

function cachedList(client: QueryClient) {
  return client.getQueryData<ReturnType<typeof makePageRow>[]>(pageKeys.list(SLUG, PAGES_PROJECT_ID)) ?? [];
}

function cachedDetail(client: QueryClient) {
  return client.getQueryData<ReturnType<typeof makePageRow>>(pageKeys.detail(SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID));
}

async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  resetRealtimeHub();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  resetRealtimeHub();
});

describe("pure cache patches", () => {
  it("patches only the matching row and leaves the collection alone when absent", () => {
    const row = makePageRow();
    expect(patchPageRow([row], OTHER_ROW, { is_locked: true })).toEqual([row]);
    expect(patchPageRow<ReturnType<typeof makePageRow>>(undefined, row.id, { is_locked: true })).toBeUndefined();
    expect(patchPageRow([row], row.id, { is_locked: true })).toEqual([{ ...row, is_locked: true }]);
  });

  it("removes only the matching row", () => {
    const row = makePageRow();
    const other = makePageRow({ id: OTHER_ROW, name: "Other" });
    expect(removePageRow([row, other], row.id)).toEqual([other]);
    expect(removePageRow(undefined, row.id)).toBeUndefined();
  });

  it("publishes onto the shared hub under the pages topic", () => {
    const seen: unknown[] = [];
    const unsubscribe = getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => seen.push(event.payload));
    publishPageEvent({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, pageId: PAGES_PAGE_ID, event: "locked" });
    expect(seen).toEqual([
      { workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, pageId: PAGES_PAGE_ID, event: "locked" },
    ]);
    unsubscribe();
    publishPageEvent({ workspaceSlug: SLUG, projectId: PAGES_PROJECT_ID, pageId: PAGES_PAGE_ID, event: "locked" });
    expect(seen).toHaveLength(1);
    expect(createRealtimeHub).toBeTypeOf("function");
  });
});

describe("usePageVisibilityWrite", () => {
  it("makes the page private, flips both caches and publishes", async () => {
    const client = newClient();
    seed(client);
    contracts.setPageAccess.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => usePageVisibilityWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.setVisibility(true);
      await settle();
    });

    expect(contracts.setPageAccess).toHaveBeenCalledWith(
      expect.anything(),
      SLUG,
      PAGES_PROJECT_ID,
      PAGES_PAGE_ID,
      PAGE_ACCESS_PRIVATE
    );
    expect(cachedList(client)[0]?.access).toBe(PAGE_ACCESS_PRIVATE);
    expect(cachedDetail(client)?.access).toBe(PAGE_ACCESS_PRIVATE);
    expect(shell.notifySuccess).toHaveBeenCalledWith("Page marked private.");
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_MADE_PRIVATE })]);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.list(SLUG, PAGES_PROJECT_ID) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.detail(SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID) });
  });

  it("makes the page public again", async () => {
    const client = newClient();
    seed(client, makePageRow({ access: PAGE_ACCESS_PRIVATE }), makePageRow({ access: PAGE_ACCESS_PRIVATE }));
    contracts.setPageAccess.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const hook = renderHook(() => usePageVisibilityWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.setVisibility(false);
      await settle();
    });

    expect(contracts.setPageAccess).toHaveBeenLastCalledWith(
      expect.anything(),
      SLUG,
      PAGES_PROJECT_ID,
      PAGES_PAGE_ID,
      PAGE_ACCESS_PUBLIC
    );
    expect(shell.notifySuccess).toHaveBeenCalledWith("Page marked public.");
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_MADE_PUBLIC })]);
  });

  it("rolls the cached access back when the server refuses", async () => {
    const client = newClient();
    seed(client);
    contracts.setPageAccess.mockRejectedValue(new Error("400"));
    const hook = renderHook(() => usePageVisibilityWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.setVisibility(true);
      await settle();
    });

    expect(cachedList(client)[0]?.access).toBe(0);
    expect(cachedDetail(client)?.access).toBe(0);
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_VISIBILITY_FAILED_TITLE);
    expect(shell.notifySuccess).not.toHaveBeenCalled();
  });

  it("ignores a second press while the first is in flight", async () => {
    const client = newClient();
    let release: (() => void) | undefined;
    contracts.setPageAccess.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const hook = renderHook(() => usePageVisibilityWrite(target()), { wrapper: wrapper(client) });
    act(() => {
      hook.result.current.setVisibility(true);
    });
    await settle();
    expect(hook.result.current.pending).toBe(true);
    act(() => {
      hook.result.current.setVisibility(false);
    });
    expect(contracts.setPageAccess).toHaveBeenCalledTimes(1);
    await act(async () => {
      release?.();
      await settle();
    });
  });
});

describe("usePageLockWrite", () => {
  it("locks the page optimistically and publishes", async () => {
    const client = newClient();
    seed(client);
    contracts.lockPage.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const hook = renderHook(() => usePageLockWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleLock(true);
      await settle();
    });

    expect(contracts.lockPage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(cachedList(client)[0]?.is_locked).toBe(true);
    expect(cachedDetail(client)?.is_locked).toBe(true);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_LOCKED_TITLE);
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_LOCKED })]);
  });

  it("unlocks through the delete route", async () => {
    const client = newClient();
    seed(client, makePageRow({ is_locked: true }), makePageRow({ is_locked: true }));
    contracts.unlockPage.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const hook = renderHook(() => usePageLockWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleLock(false);
      await settle();
    });

    expect(contracts.unlockPage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(cachedList(client)[0]?.is_locked).toBe(false);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_UNLOCKED_TITLE);
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_UNLOCKED })]);
  });

  it("rolls the lock back and reports a refusal", async () => {
    const client = newClient();
    seed(client);
    contracts.lockPage.mockRejectedValue(new Error("nope"));
    const hook = renderHook(() => usePageLockWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleLock(true);
      await settle();
    });

    expect(cachedList(client)[0]?.is_locked).toBe(false);
    expect(cachedDetail(client)?.is_locked).toBe(false);
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_LOCK_FAILED_TITLE);
  });

  it("skips a second toggle while locked write is pending", async () => {
    const client = newClient();
    let release: (() => void) | undefined;
    contracts.lockPage.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const hook = renderHook(() => usePageLockWrite(target()), { wrapper: wrapper(client) });
    act(() => {
      hook.result.current.toggleLock(true);
    });
    await settle();
    act(() => {
      hook.result.current.toggleLock(false);
    });
    expect(contracts.unlockPage).not.toHaveBeenCalled();
    await act(async () => {
      release?.();
      await settle();
    });
  });
});

describe("usePageArchiveWrite", () => {
  it("shelves the page, drops its favourite and publishes", async () => {
    const client = newClient();
    seed(client, makePageRow({ is_favorite: true }), makePageRow({ is_favorite: true }));
    contracts.archivePage.mockResolvedValue({ archived_at: "2026-10-25T10:00:00" });
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const hook = renderHook(() => usePageArchiveWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleArchive(true);
      await settle();
    });

    expect(contracts.archivePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    const archived = cachedList(client)[0];
    expect(archived?.archived_at).not.toBeNull();
    expect(archived?.is_favorite).toBe(false);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_ARCHIVED_TITLE);
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_ARCHIVED })]);
  });

  it("restores a shelved page", async () => {
    const client = newClient();
    seed(client, makePageRow({ archived_at: "2026-10-01" }), makePageRow({ archived_at: "2026-10-01" }));
    contracts.restorePage.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const hook = renderHook(() => usePageArchiveWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleArchive(false);
      await settle();
    });

    expect(contracts.restorePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(cachedList(client)[0]?.archived_at).toBeNull();
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_RESTORED_TITLE);
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_UNARCHIVED })]);
  });

  it("rolls the archive stamp back on refusal", async () => {
    const client = newClient();
    seed(client);
    contracts.archivePage.mockRejectedValue(new Error("nope"));
    const hook = renderHook(() => usePageArchiveWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleArchive(true);
      await settle();
    });

    expect(cachedList(client)[0]?.archived_at).toBeNull();
    expect(cachedDetail(client)?.archived_at).toBeNull();
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_ARCHIVE_FAILED_TITLE);
  });

  it("reports a failed restore with its own copy", async () => {
    const client = newClient();
    seed(client, makePageRow({ archived_at: "2026-10-01" }), makePageRow({ archived_at: "2026-10-01" }));
    contracts.restorePage.mockRejectedValue(new Error("nope"));
    const hook = renderHook(() => usePageArchiveWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.toggleArchive(false);
      await settle();
    });

    expect(cachedList(client)[0]?.archived_at).toBe("2026-10-01");
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_RESTORE_FAILED_TITLE);
  });

  it("skips a second archive toggle while the first flies", async () => {
    const client = newClient();
    let release: (() => void) | undefined;
    contracts.archivePage.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const hook = renderHook(() => usePageArchiveWrite(target()), { wrapper: wrapper(client) });
    act(() => {
      hook.result.current.toggleArchive(true);
    });
    await settle();
    act(() => {
      hook.result.current.toggleArchive(false);
    });
    expect(contracts.restorePage).not.toHaveBeenCalled();
    await act(async () => {
      release?.();
      await settle();
    });
  });
});

describe("usePageDeleteWrite", () => {
  it("removes the row once confirmed and calls back", async () => {
    const client = newClient();
    seed(client);
    contracts.deletePage.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const onDeleted = vi.fn();
    const hook = renderHook(() => usePageDeleteWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.remove({ onDeleted });
      await settle();
    });

    expect(contracts.deletePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID);
    expect(cachedList(client).map((row) => row.id)).toEqual([OTHER_ROW]);
    expect(cachedDetail(client)).toBeUndefined();
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_DELETED_TITLE);
    expect(onDeleted).toHaveBeenCalledTimes(1);
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_DELETED })]);
  });

  it("leaves the row in place when the delete is refused", async () => {
    const client = newClient();
    seed(client);
    contracts.deletePage.mockRejectedValue(new Error("400"));
    const onDeleted = vi.fn();
    const hook = renderHook(() => usePageDeleteWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.remove({ onDeleted });
      await settle();
    });

    expect(cachedList(client).map((row) => row.id)).toEqual([PAGES_PAGE_ID, OTHER_ROW]);
    expect(cachedDetail(client)?.id).toBe(PAGES_PAGE_ID);
    expect(onDeleted).not.toHaveBeenCalled();
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_DELETE_FAILED_TITLE);
  });

  it("works without a callback and ignores a press while pending", async () => {
    const client = newClient();
    let reject: ((error: Error) => void) | undefined;
    contracts.deletePage.mockImplementation(
      () =>
        new Promise<void>((_resolve, fail) => {
          reject = fail;
        })
    );
    const hook = renderHook(() => usePageDeleteWrite(target()), { wrapper: wrapper(client) });
    act(() => {
      hook.result.current.remove();
    });
    await settle();
    expect(hook.result.current.pending).toBe(true);
    act(() => {
      hook.result.current.remove();
    });
    expect(contracts.deletePage).toHaveBeenCalledTimes(1);
    await act(async () => {
      reject?.(new Error("nope"));
      await settle();
    });
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_DELETE_FAILED_TITLE);
  });
});

describe("usePageMoveWrite", () => {
  it("moves the page out of the collection and publishes", async () => {
    const client = newClient();
    seed(client);
    contracts.movePageToProject.mockResolvedValue(undefined);
    const events: unknown[] = [];
    getRealtimeHub().subscribe(PAGES_REALTIME_TOPIC, (event) => events.push(event.payload));
    const hook = renderHook(() => usePageMoveWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.move(OTHER_PROJECT);
      await settle();
    });

    expect(contracts.movePageToProject).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID, {
      new_project_id: OTHER_PROJECT,
    });
    expect(cachedList(client).map((row) => row.id)).toEqual([OTHER_ROW]);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_MOVED_TITLE);
    expect(events).toEqual([expect.objectContaining({ event: PAGE_EVENT_MOVED })]);
  });

  it("keeps the row when the move fails", async () => {
    const client = newClient();
    seed(client);
    contracts.movePageToProject.mockRejectedValue(new Error("404"));
    const hook = renderHook(() => usePageMoveWrite(target()), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.move(OTHER_PROJECT);
      await settle();
    });

    expect(cachedList(client).map((row) => row.id)).toEqual([PAGES_PAGE_ID, OTHER_ROW]);
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_MOVE_FAILED_TITLE);
  });

  it("ignores a second move while one is in flight", async () => {
    const client = newClient();
    let release: (() => void) | undefined;
    contracts.movePageToProject.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const hook = renderHook(() => usePageMoveWrite(target()), { wrapper: wrapper(client) });
    act(() => {
      hook.result.current.move(OTHER_PROJECT);
    });
    await settle();
    act(() => {
      hook.result.current.move(OTHER_PROJECT);
    });
    expect(contracts.movePageToProject).toHaveBeenCalledTimes(1);
    await act(async () => {
      release?.();
      await settle();
    });
    await waitFor(() => expect(shell.notifySuccess).toHaveBeenCalled());
  });
});
