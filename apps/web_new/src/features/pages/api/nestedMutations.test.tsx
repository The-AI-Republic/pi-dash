// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested tree writes (parity/specs/pages.md §11.7, PAGE-084/087/088): the
// sub-page create sends the source parent plus the caller's quick-create
// visibility and refreshes the parent's children, the reparent moves one
// page and reports the server's refusal verbatim, and the subtree archive
// chooses the restore-to-top notice from the caller's collection.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { ApiError, PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({
  createPage: vi.fn(),
  updatePage: vi.fn(),
  archivePage: vi.fn(),
  restorePage: vi.fn(),
}));

const shell = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  ...contracts,
}));
vi.mock("../../../shared/shell/toaster.js", () => shell);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { pageKeys } from "./keys.js";
import {
  moveFailureMessage,
  PAGE_SUB_PAGE_NAME,
  useAddSubPage,
  usePageReparent,
  usePageSubtreeArchive,
} from "./nestedMutations.js";
import { PAGE_MOVE_FAILED_TITLE, PAGE_ARCHIVED_TITLE, PAGE_RESTORED_TITLE } from "../lib/pageActions.js";
import { PAGE_NESTED_MOVED_TITLE, PAGE_RESTORE_TO_TOP_TITLE } from "../lib/pageNested.js";
import { PAGES_PAGE_ID, PAGES_PROJECT_ID, makePageRow } from "../testing.js";

const SLUG = "acme";
const PARENT = "923e4567-e89b-12d3-a456-426614174009";

function newClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 30));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("moveFailureMessage", () => {
  it("prefers the server's own refusal text", () => {
    const refused = new ApiError({ status: 400, code: "http", message: "A page cannot be nested under itself" });
    expect(moveFailureMessage(refused)).toBe("A page cannot be nested under itself");
    expect(moveFailureMessage(new Error("x"))).toBe(PAGE_MOVE_FAILED_TITLE);
    expect(moveFailureMessage(new ApiError({ status: 400, code: "http", message: "   " }))).toBe(
      PAGE_MOVE_FAILED_TITLE
    );
    expect(moveFailureMessage("nope")).toBe(PAGE_MOVE_FAILED_TITLE);
  });
});

describe("useAddSubPage", () => {
  it("creates an untitled child under the source with its visibility and refreshes the tree", async () => {
    const client = newClient();
    contracts.createPage.mockResolvedValue({ ...makePageRow({ name: PAGE_SUB_PAGE_NAME }), description_html: "" });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => useAddSubPage(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.mutate({ parentId: PARENT, access: PAGE_ACCESS_PRIVATE });
      await settle();
    });

    expect(contracts.createPage).toHaveBeenCalledWith(
      expect.anything(),
      SLUG,
      PAGES_PROJECT_ID,
      expect.objectContaining({ name: PAGE_SUB_PAGE_NAME, parent: PARENT, access: PAGE_ACCESS_PRIVATE })
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: [...pageKeys.all(SLUG), "children", PAGES_PROJECT_ID, PARENT],
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.all(SLUG) });
    expect(hook.result.current.data?.name).toBe(PAGE_SUB_PAGE_NAME);
    expect(hook.result.current.data).toBeDefined();
    expect(shell.notifyError).not.toHaveBeenCalled();
    expect(PAGE_ACCESS_PUBLIC).toBe(0);
  });

  it("reports a failed create", async () => {
    const client = newClient();
    contracts.createPage.mockRejectedValue(new Error("boom"));
    const hook = renderHook(() => useAddSubPage(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });
    await act(async () => {
      hook.result.current.mutate({ parentId: PARENT, access: PAGE_ACCESS_PUBLIC });
      await settle();
    });
    expect(shell.notifyError).toHaveBeenCalledWith("Page could not be created.");
  });
});

describe("usePageReparent", () => {
  it("patches the parent, notices and invalidates the nested subtree", async () => {
    const client = newClient();
    contracts.updatePage.mockResolvedValue(makePageRow());
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => usePageReparent(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });

    await act(async () => {
      hook.result.current.mutate({ pageId: PAGES_PAGE_ID, parentId: PARENT });
      await settle();
    });

    expect(contracts.updatePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID, {
      parent: PARENT,
    });
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_NESTED_MOVED_TITLE);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.all(SLUG) });
  });

  it("detaches to the top level with a null parent", async () => {
    const client = newClient();
    contracts.updatePage.mockResolvedValue(makePageRow());
    const hook = renderHook(() => usePageReparent(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });
    await act(async () => {
      hook.result.current.mutate({ pageId: PAGES_PAGE_ID, parentId: null });
      await settle();
    });
    expect(contracts.updatePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, PAGES_PAGE_ID, {
      parent: null,
    });
  });

  it("reports a refused move with the server's reason", async () => {
    const client = newClient();
    contracts.updatePage.mockRejectedValue(new ApiError({ status: 400, code: "http", message: "Page is archived" }));
    const hook = renderHook(() => usePageReparent(SLUG, PAGES_PROJECT_ID), { wrapper: wrapper(client) });
    await act(async () => {
      hook.result.current.mutate({ pageId: PAGES_PAGE_ID, parentId: PARENT });
      await settle();
    });
    expect(shell.notifyError).toHaveBeenCalledWith("Page is archived");
  });
});

describe("usePageSubtreeArchive", () => {
  const archivedParent = makePageRow({ id: PARENT, archived_at: "2026-10-20T00:00:00Z" });
  const child = makePageRow({ id: PAGES_PAGE_ID, parent: PARENT, parent_id: PARENT });

  it("archives, notices and hands the new state back", async () => {
    const client = newClient();
    contracts.archivePage.mockResolvedValue(undefined);
    const onChange = vi.fn();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const hook = renderHook(() => usePageSubtreeArchive(SLUG, PAGES_PROJECT_ID, [archivedParent, child], onChange), {
      wrapper: wrapper(client),
    });
    await act(async () => {
      hook.result.current.mutate({ pageId: child.id, row: child, archived: true });
      await settle();
    });
    expect(contracts.archivePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, child.id);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_ARCHIVED_TITLE);
    expect(onChange).toHaveBeenCalledWith(true);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pageKeys.all(SLUG) });
  });

  it("says a restore went to the top level when the parent is still archived", async () => {
    const client = newClient();
    contracts.restorePage.mockResolvedValue(undefined);
    const hook = renderHook(() => usePageSubtreeArchive(SLUG, PAGES_PROJECT_ID, [archivedParent, child]), {
      wrapper: wrapper(client),
    });
    await act(async () => {
      hook.result.current.mutate({ pageId: child.id, row: child, archived: false });
      await settle();
    });
    expect(contracts.restorePage).toHaveBeenCalledWith(expect.anything(), SLUG, PAGES_PROJECT_ID, child.id);
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_RESTORE_TO_TOP_TITLE);
  });

  it("uses the ordinary restore notice when the parent is live", async () => {
    const client = newClient();
    contracts.restorePage.mockResolvedValue(undefined);
    const liveParent = makePageRow({ id: PARENT });
    const hook = renderHook(() => usePageSubtreeArchive(SLUG, PAGES_PROJECT_ID, [liveParent, child]), {
      wrapper: wrapper(client),
    });
    await act(async () => {
      hook.result.current.mutate({ pageId: child.id, row: child, archived: false });
      await settle();
    });
    expect(shell.notifySuccess).toHaveBeenCalledWith(PAGE_RESTORED_TITLE);
  });

  it("reports archive and restore failures separately", async () => {
    const client = newClient();
    contracts.archivePage.mockRejectedValueOnce(new Error("no"));
    contracts.restorePage.mockRejectedValueOnce(new Error("no"));
    const hook = renderHook(() => usePageSubtreeArchive(SLUG, PAGES_PROJECT_ID, [archivedParent, child]), {
      wrapper: wrapper(client),
    });
    await act(async () => {
      hook.result.current.mutate({ pageId: child.id, row: child, archived: true });
      await settle();
    });
    expect(shell.notifyError).toHaveBeenLastCalledWith("Page could not be archived.");
    await act(async () => {
      hook.result.current.mutate({ pageId: child.id, row: child, archived: false });
      await settle();
    });
    expect(shell.notifyError).toHaveBeenLastCalledWith("Page could not be restored.");
  });
});
