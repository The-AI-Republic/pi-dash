// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested tree reads (parity/specs/pages.md §11.7, PAGE-085): one `?parent=`
// read per expanded parent, a breadth-first forest walk that excludes what
// the caller cannot see, a ceiling on the walk, and top-level roots for a
// pool whose parents are missing.

import { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { makePageRow } from "../testing.js";
import { pageKeys } from "./keys.js";
import {
  PAGE_FOREST_LIMIT,
  pageChildrenQueryOptions,
  pageForestKey,
  pageForestQueryOptions,
  readPageForest,
  rootsOf,
} from "./treeQueries.js";
import { listPages } from "@pidash/api-client";

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  listPages: vi.fn(),
}));

const listPagesMock = vi.mocked(listPages);

beforeEach(() => {
  listPagesMock.mockReset();
});

function fakeClient(): ApiClient {
  return {} as ApiClient;
}

function row(name: string, extra: Parameters<typeof makePageRow>[0] = {}) {
  return makePageRow({ name, ...extra });
}

describe("pageChildrenQueryOptions", () => {
  it("keys children under their parent and asks the contract for that parent", async () => {
    const options = pageChildrenQueryOptions(fakeClient(), "acme", "project", "page-1");
    expect(options.queryKey).toEqual(pageKeys.children("acme", "project", "page-1"));
    listPagesMock.mockResolvedValueOnce([row("Child")]);
    await options.queryFn?.({} as never);
    expect(listPagesMock).toHaveBeenCalledWith(expect.anything(), "acme", "project", {
      parent: "page-1",
      includeArchived: false,
    });
  });

  it("asks for archived children and keeps their cache key separate", async () => {
    const options = pageChildrenQueryOptions(fakeClient(), "acme", "project", "page-1", true);
    expect(options.queryKey).toEqual(pageKeys.children("acme", "project", "page-1", true));
    listPagesMock.mockResolvedValueOnce([row("Shelved child", { archived_at: "2026-10-20" })]);
    await options.queryFn?.({} as never);
    expect(listPagesMock).toHaveBeenCalledWith(expect.anything(), "acme", "project", {
      parent: "page-1",
      includeArchived: true,
    });
  });
});

describe("readPageForest", () => {
  it("walks the visible tree breadth-first and never asks for a childless parent", async () => {
    const root = row("Root", { id: "root", sub_pages_count: 1 });
    const child = row("Child", { id: "child", parent: "root", parent_id: "root", sub_pages_count: 1 });
    const grandchild = row("Grandchild", { id: "grand", parent: "child", parent_id: "child" });
    const leaf = row("Leaf", { id: "leaf" });
    listPagesMock.mockImplementation(async (_client, _ws, _project, query) => {
      if (query?.parent === undefined) return [root, leaf];
      if (query.parent === "root") return [child];
      if (query.parent === "child") return [grandchild];
      return [];
    });
    const forest = await readPageForest(fakeClient(), "acme", "project");
    expect(forest.complete).toBe(true);
    expect(forest.rows.map((page) => page.id)).toEqual(["root", "leaf", "child", "grand"]);
    expect(listPagesMock.mock.calls.map((call) => call[3]?.parent).sort()).toEqual([undefined, "child", "root"].sort());
    expect(listPagesMock.mock.calls.every((call) => call[3]?.includeArchived === true)).toBe(true);
  });

  it("skips a child already seen so a malformed cycle terminates", async () => {
    const root = row("Root", { id: "root", sub_pages_count: 1 });
    const child = row("Child", { id: "child", parent: "root", parent_id: "root", sub_pages_count: 1 });
    listPagesMock.mockImplementation(async (_client, _ws, _project, query) => {
      if (query?.parent === undefined) return [root];
      if (query.parent === "root") return [child];
      if (query.parent === "child") return [root];
      return [];
    });
    const forest = await readPageForest(fakeClient(), "acme", "project");
    expect(forest.rows.map((page) => page.id)).toEqual(["root", "child"]);
  });

  it("stops at the ceiling and reports the walk as incomplete", async () => {
    const roots = Array.from({ length: PAGE_FOREST_LIMIT }, (_, index) =>
      row(`Root ${index}`, { id: `root-${index}`, sub_pages_count: 1 })
    );
    listPagesMock.mockImplementation(async (_client, _ws, _project, query) => {
      if (query?.parent === undefined) return roots;
      return [];
    });
    const forest = await readPageForest(fakeClient(), "acme", "project");
    expect(forest.complete).toBe(false);
    expect(forest.rows).toHaveLength(PAGE_FOREST_LIMIT);
  });

  it("keys and enables the forest read", () => {
    const options = pageForestQueryOptions(fakeClient(), "acme", "project", false);
    expect(options.queryKey).toEqual(pageForestKey("acme", "project"));
    expect(options.enabled).toBe(false);
    expect(pageForestKey("acme", "project")).toEqual([...pageKeys.all("acme"), "forest", "project"]);
  });

  it("resolves the query through a QueryClient", async () => {
    listPagesMock.mockResolvedValueOnce([row("Root", { id: "root" })]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = pageForestQueryOptions(fakeClient(), "acme", "project", true);
    const data = await client.fetchQuery(options);
    expect(data.rows.map((page) => page.name)).toEqual(["Root"]);
  });
});

describe("rootsOf", () => {
  it("keeps top-level rows and rows whose parent is absent from the pool", () => {
    const top = row("Top", { id: "top" });
    const child = row("Child", { id: "child", parent: "top", parent_id: "top" });
    const orphan = row("Orphan", { id: "orphan", parent: "gone", parent_id: "gone" });
    expect(rootsOf([top, child, orphan]).map((page) => page.id)).toEqual(["top", "orphan"]);
  });
});
