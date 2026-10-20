// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived page's read and its window-local expression (parity/specs
// issues.md §8, ISS-071; NEWFRONT-167). Reads take the open page's
// expression and order; the local store restores one project's filters
// without ever touching another project's entry.
import { describe, expect, it } from "vitest";

import { createAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";

import {
  ARCHIVED_FILTER_STORAGE_KEY,
  archivedIssuesQueryOptions,
  projectAutomationsPath,
  readArchivedLocalFilter,
  writeArchivedLocalFilter,
} from "./archivedIssues.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "323e4567-e89b-12d3-a456-426614174002";
const EXPRESSION = { and: [{ state_group__in: ["cancelled"] }] };

function memoryStorage(seed: Record<string, string> = {}) {
  const data = new Map(Object.entries(seed));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    removeItem: (key: string) => void data.delete(key),
  } as unknown as Storage & { data: Map<string, string> };
}

describe("archivedIssuesQueryOptions", () => {
  it("reads the archived list with the expression and order it is given", async () => {
    const seen: string[] = [];
    const client = createAppClient("https://api.test", (async (url: string) => {
      seen.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            grouped_by: null,
            sub_grouped_by: null,
            total_count: 0,
            count: 0,
            total_results: 0,
            results: [],
            next_cursor: "",
            prev_cursor: "",
            next_page_results: false,
            prev_page_results: false,
          }),
      };
    }) as never);
    const options = archivedIssuesQueryOptions(client, SLUG, PROJECT, EXPRESSION as never, "-created_at");
    expect(options.queryKey).toEqual(
      issueKeys.lists(SLUG, "archived", PROJECT).concat(["-created_at", expect.any(String)])
    );
    const read = options.queryFn as (context: unknown) => Promise<unknown>;
    await read({ client, queryKey: options.queryKey } as never);
    expect(seen).toHaveLength(1);
    const url = new URL(seen[0] ?? "");
    expect(url.pathname).toContain("/archived-issues/");
    expect(url.searchParams.get("order_by")).toBe("-created_at");
    expect(url.searchParams.get("cursor")).toBeNull();
    expect(decodeURIComponent(seen[0] ?? "")).toContain("state_group");
  });

  it("sends no expression params for an empty expression", async () => {
    const seen: string[] = [];
    const client = createAppClient("https://api.test", (async (url: string) => {
      seen.push(url);
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            grouped_by: null,
            sub_grouped_by: null,
            total_count: 0,
            count: 0,
            total_results: 0,
            results: [],
            next_cursor: "",
            prev_cursor: "",
            next_page_results: false,
            prev_page_results: false,
          }),
      };
    }) as never);
    const options = archivedIssuesQueryOptions(client, SLUG, PROJECT, {}, "-created_at");
    const read = options.queryFn as (context: unknown) => Promise<unknown>;
    await read({ client, queryKey: options.queryKey } as never);
    expect(seen[0]).not.toContain("filters");
  });
});

describe("projectAutomationsPath", () => {
  it("points at the project's automation settings", () => {
    expect(projectAutomationsPath(SLUG, PROJECT)).toBe(`/${SLUG}/settings/projects/${PROJECT}/automations`);
  });
});

describe("readArchivedLocalFilter", () => {
  it("reads the entry written for this workspace and project", () => {
    const storage = memoryStorage({
      [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify([
        { key: "ARCHIVED", workspaceSlug: SLUG, viewId: PROJECT, filters: { richFilters: EXPRESSION } },
      ]),
    });
    expect(readArchivedLocalFilter(SLUG, PROJECT, storage)).toEqual(EXPRESSION);
  });

  it("reads as empty without storage or without an entry", () => {
    expect(readArchivedLocalFilter(SLUG, PROJECT, undefined)).toEqual({});
    expect(readArchivedLocalFilter(SLUG, PROJECT, memoryStorage())).toEqual({});
  });

  it("skips corrupt JSON and non-array records", () => {
    expect(readArchivedLocalFilter(SLUG, PROJECT, memoryStorage({ [ARCHIVED_FILTER_STORAGE_KEY]: "{" }))).toEqual({});
    expect(readArchivedLocalFilter(SLUG, PROJECT, memoryStorage({ [ARCHIVED_FILTER_STORAGE_KEY]: '"x"' }))).toEqual({});
  });

  it("ignores entries for another project, workspace or key", () => {
    const storage = memoryStorage({
      [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify([
        null,
        "nope",
        { key: "OTHER", workspaceSlug: SLUG, viewId: PROJECT, filters: { richFilters: EXPRESSION } },
        { key: "ARCHIVED", workspaceSlug: SLUG, viewId: OTHER, filters: { richFilters: EXPRESSION } },
        { key: "ARCHIVED", workspaceSlug: "elsewhere", viewId: PROJECT, filters: { richFilters: EXPRESSION } },
      ]),
    });
    expect(readArchivedLocalFilter(SLUG, PROJECT, storage)).toEqual({});
  });

  it("reads a non-object expression or missing filters as no filter", () => {
    const nonObject = memoryStorage({
      [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify([
        { key: "ARCHIVED", workspaceSlug: SLUG, viewId: PROJECT, filters: { richFilters: ["x"] } },
      ]),
    });
    expect(readArchivedLocalFilter(SLUG, PROJECT, nonObject)).toEqual({});
    const missing = memoryStorage({
      [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify([{ key: "ARCHIVED", workspaceSlug: SLUG, viewId: PROJECT }]),
    });
    expect(readArchivedLocalFilter(SLUG, PROJECT, missing)).toEqual({});
  });

  it("falls back to the window's storage when no store is passed", () => {
    window.localStorage.setItem(
      ARCHIVED_FILTER_STORAGE_KEY,
      JSON.stringify([{ key: "ARCHIVED", workspaceSlug: SLUG, viewId: PROJECT, filters: { richFilters: EXPRESSION } }])
    );
    try {
      expect(readArchivedLocalFilter(SLUG, PROJECT)).toEqual(EXPRESSION);
    } finally {
      window.localStorage.removeItem(ARCHIVED_FILTER_STORAGE_KEY);
    }
  });
});

describe("writeArchivedLocalFilter", () => {
  it("writes one project's entry and keeps the rest", () => {
    const storage = memoryStorage({
      [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify([
        { key: "ARCHIVED", workspaceSlug: SLUG, viewId: OTHER, filters: { richFilters: { and: [] } } },
        { key: "SOMETHING", value: 1 },
      ]),
    });
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    const stored = JSON.parse(storage.data.get(ARCHIVED_FILTER_STORAGE_KEY) ?? "[]") as unknown[];
    expect(stored).toEqual([
      { key: "ARCHIVED", workspaceSlug: SLUG, viewId: OTHER, filters: { richFilters: { and: [] } } },
      { key: "SOMETHING", value: 1 },
      { key: "ARCHIVED", workspaceSlug: SLUG, viewId: PROJECT, filters: { richFilters: EXPRESSION } },
    ]);
    expect(readArchivedLocalFilter(SLUG, PROJECT, storage)).toEqual(EXPRESSION);
  });

  it("replaces the same project's previous entry", () => {
    const storage = memoryStorage();
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    writeArchivedLocalFilter(SLUG, PROJECT, { and: [] } as never, storage);
    const stored = JSON.parse(storage.data.get(ARCHIVED_FILTER_STORAGE_KEY) ?? "[]") as unknown[];
    expect(stored).toHaveLength(1);
    expect(readArchivedLocalFilter(SLUG, PROJECT, storage)).toEqual({ and: [] });
  });

  it("drops the entry when the expression is cleared", () => {
    const storage = memoryStorage();
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    writeArchivedLocalFilter(SLUG, PROJECT, {}, storage);
    expect(storage.data.has(ARCHIVED_FILTER_STORAGE_KEY)).toBe(false);
  });

  it("keeps other entries when one project clears and others remain", () => {
    const storage = memoryStorage();
    writeArchivedLocalFilter(SLUG, OTHER, EXPRESSION as never, storage);
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    writeArchivedLocalFilter(SLUG, PROJECT, {}, storage);
    const stored = JSON.parse(storage.data.get(ARCHIVED_FILTER_STORAGE_KEY) ?? "[]") as unknown[];
    expect(stored).toEqual([
      { key: "ARCHIVED", workspaceSlug: SLUG, viewId: OTHER, filters: { richFilters: EXPRESSION } },
    ]);
  });

  it("replaces a corrupt record instead of failing", () => {
    const storage = memoryStorage({ [ARCHIVED_FILTER_STORAGE_KEY]: "{" });
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    expect(readArchivedLocalFilter(SLUG, PROJECT, storage)).toEqual(EXPRESSION);
  });

  it("drops junk entries from a non-array record", () => {
    const storage = memoryStorage({ [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify({ key: "ARCHIVED" }) });
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    const stored = JSON.parse(storage.data.get(ARCHIVED_FILTER_STORAGE_KEY) ?? "[]") as unknown[];
    expect(stored).toHaveLength(1);
  });

  it("does nothing without storage", () => {
    expect(() => writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, undefined)).not.toThrow();
  });

  it("falls back to the window's storage when no store is passed", () => {
    window.localStorage.removeItem(ARCHIVED_FILTER_STORAGE_KEY);
    try {
      writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never);
      expect(window.localStorage.getItem(ARCHIVED_FILTER_STORAGE_KEY)).toContain("ARCHIVED");
    } finally {
      window.localStorage.removeItem(ARCHIVED_FILTER_STORAGE_KEY);
    }
  });

  it("filters junk entries out of the kept list", () => {
    const storage = memoryStorage({ [ARCHIVED_FILTER_STORAGE_KEY]: JSON.stringify([null, "x", 3]) });
    writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION as never, storage);
    const stored = JSON.parse(storage.data.get(ARCHIVED_FILTER_STORAGE_KEY) ?? "[]") as unknown[];
    expect(stored).toHaveLength(1);
  });
  it("falls back to no expression when the browser's localStorage throws", () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get: () => {
        throw new Error("blocked");
      },
    });
    try {
      expect(readArchivedLocalFilter(SLUG, PROJECT)).toEqual({});
      expect(() => writeArchivedLocalFilter(SLUG, PROJECT, EXPRESSION)).not.toThrow();
    } finally {
      if (descriptor !== undefined) Object.defineProperty(window, "localStorage", descriptor);
      else Reflect.deleteProperty(window, "localStorage");
    }
  });
});
