// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The desktop gates must recognise exactly the payload the reload can paint
// from (NEWFRONT-886, NEWFRONT-894). Each row-count case below is a real
// shape the persistor can leave behind; the reader cases exercise the
// storage read both specs take before parsing.
// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";

import { cachedIssueListRows, desktopCachePayload } from "./desktopCache.js";

const SLUG = "perf-ws";
const PROJECT = "123e4567-e89b-42d3-a456-426614174001";
const OTHER_PROJECT = "123e4567-e89b-42d3-a456-426614174099";

function listKey(projectId: string = PROJECT, layout: string = "list"): unknown[] {
  return ["ws", SLUG, "issues", "list", projectId, { layout }, "sort_order"];
}

function payload(entries: unknown[]): string {
  return JSON.stringify({ version: 1, state: { queries: entries, mutations: [] } });
}

function listEntry(options: { projectId?: string; status?: string; rows?: unknown[]; data?: unknown } = {}) {
  const rows = options.rows ?? [{ id: "i1" }];
  return {
    queryKey: listKey(options.projectId),
    state: {
      status: options.status ?? "success",
      data: options.data ?? { results: rows, count: rows.length },
    },
  };
}

describe("cachedIssueListRows", () => {
  it("counts the rows of this project's list entry", () => {
    expect(cachedIssueListRows(payload([listEntry({ rows: [{ id: "a" }, { id: "b" }, { id: "c" }] })]), PROJECT)).toBe(
      3
    );
  });

  it("returns zero when the payload holds bootstrap queries but no issues entry", () => {
    const bootstrapOnly = payload([
      { queryKey: ["ws", SLUG, "reference", "states"], state: { status: "success", data: [] } },
      { queryKey: ["ws", SLUG, "me"], state: { status: "success", data: { id: "u1" } } },
    ]);
    expect(bootstrapOnly.length).toBeGreaterThan(0);
    expect(cachedIssueListRows(bootstrapOnly, PROJECT)).toBe(0);
  });

  it("returns zero when the baked list is empty or still pending", () => {
    expect(cachedIssueListRows(payload([listEntry({ rows: [] })]), PROJECT)).toBe(0);
    expect(cachedIssueListRows(payload([listEntry({ status: "pending" })]), PROJECT)).toBe(0);
  });

  it("accepts an errored entry that still carries fetched rows", () => {
    expect(cachedIssueListRows(payload([listEntry({ status: "error", rows: [{ id: "a" }] })]), PROJECT)).toBe(1);
  });

  it("ignores another project's list and non-list issue queries", () => {
    const otherProject = payload([listEntry({ projectId: OTHER_PROJECT, rows: [{ id: "a" }, { id: "b" }] })]);
    expect(cachedIssueListRows(otherProject, PROJECT)).toBe(0);
    const detailOnly = payload([
      {
        queryKey: ["ws", SLUG, "issues", "detail", PROJECT, "issue-1"],
        state: { status: "success", data: { id: "i1" } },
      },
    ]);
    expect(cachedIssueListRows(detailOnly, PROJECT)).toBe(0);
  });

  it("takes the fullest view when the list is cached under several shapes", () => {
    const stored = payload([
      listEntry({ rows: [{ id: "a" }] }),
      {
        queryKey: listKey(PROJECT, "compact"),
        state: { status: "success", data: { results: [{ id: "a" }, { id: "b" }] } },
      },
    ]);
    expect(cachedIssueListRows(stored, PROJECT)).toBe(2);
  });

  it("returns zero for missing, empty, malformed and off-shape payloads", () => {
    expect(cachedIssueListRows(null, PROJECT)).toBe(0);
    expect(cachedIssueListRows("", PROJECT)).toBe(0);
    expect(cachedIssueListRows("{not json", PROJECT)).toBe(0);
    expect(cachedIssueListRows("null", PROJECT)).toBe(0);
    expect(cachedIssueListRows(JSON.stringify({ version: 1 }), PROJECT)).toBe(0);
    expect(cachedIssueListRows(JSON.stringify({ state: { queries: "nope" } }), PROJECT)).toBe(0);
  });

  it("skips entries that do not carry a list payload", () => {
    const entries = [
      null,
      "string-entry",
      [],
      { queryKey: "ws", state: { status: "success", data: { results: [{}] } } },
      { queryKey: ["ws", SLUG, "issues", "list"], state: { status: "success", data: { results: [{}] } } },
      { queryKey: listKey(), state: { status: "success" } },
      { queryKey: listKey(), state: { status: "success", data: null } },
      { queryKey: listKey(), state: { status: "success", data: { results: null } } },
      { queryKey: listKey(), state: "settled" },
      listEntry({ rows: [{ id: "a" }, { id: "b" }] }),
    ];
    expect(cachedIssueListRows(payload(entries), PROJECT)).toBe(2);
  });

  it("does not mistake a scoped non-workspace key or a reference list for cached rows", () => {
    const entries = [
      {
        queryKey: ["scope", SLUG, "issues", "list", PROJECT, {}, "sort_order"],
        state: { status: "success", data: { results: [{}] } },
      },
      {
        queryKey: ["ws", SLUG, "reference", "path", PROJECT, {}, "sort_order"],
        state: { status: "success", data: { results: [{}] } },
      },
    ];
    expect(cachedIssueListRows(payload(entries), PROJECT)).toBe(0);
  });
});

type FakeHandler = () => void;

interface FakeRequest {
  result: unknown;
  onsuccess: FakeHandler | null;
  onerror: FakeHandler | null;
}

/** A browser-style request whose handler fires once, after the caller attached it. */
function fakeRequest(fire: (request: FakeRequest) => void): FakeRequest {
  const request: FakeRequest = { result: undefined, onsuccess: null, onerror: null };
  queueMicrotask(() => fire(request));
  return request;
}

function succeed(request: FakeRequest, result: unknown): void {
  request.result = result;
  request.onsuccess?.();
}

/** A page IndexedDB whose `open` succeeds into the given store. */
function fakeIndexedDb(options: {
  hasStore?: boolean;
  read?: (key: string) => FakeRequest;
  openFails?: boolean;
  close?: () => void;
}): unknown {
  const close = options.close ?? (() => undefined);
  return {
    open: () => {
      if (options.openFails === true) return fakeRequest((request) => request.onerror?.());
      return fakeRequest((request) => {
        request.result = {
          objectStoreNames: { contains: () => options.hasStore ?? true },
          transaction: () => ({
            objectStore: () => ({ get: options.read ?? (() => fakeRequest(() => undefined)) }),
          }),
          close,
        };
        request.onsuccess?.();
      });
    },
  };
}

const STORED = '{"version":1,"state":{"queries":[]}}';

describe("desktopCachePayload", () => {
  const originalIndexedDb = (globalThis as { indexedDB?: unknown }).indexedDB;
  const setIndexedDb = (value: unknown): void => {
    (globalThis as { indexedDB?: unknown }).indexedDB = value;
  };

  afterEach(() => {
    (globalThis as { indexedDB?: unknown }).indexedDB = originalIndexedDb;
    vi.restoreAllMocks();
  });

  it("returns the string stored under the desktop cache key", async () => {
    const get = vi.fn((_key: string) => fakeRequest((request) => succeed(request, STORED)));
    setIndexedDb(fakeIndexedDb({ read: get }));

    expect(await desktopCachePayload()).toBe(STORED);
    expect(get).toHaveBeenCalledWith("pidash.desktop:tanstack-query-cache");
  });

  it("reads nothing when the store holds a non-string value", async () => {
    setIndexedDb(fakeIndexedDb({ read: () => fakeRequest((request) => succeed(request, { rows: [] })) }));

    expect(await desktopCachePayload()).toBeNull();
  });

  it("reads nothing when the database has no key-value store", async () => {
    const close = vi.fn();
    setIndexedDb(fakeIndexedDb({ hasStore: false, close }));

    expect(await desktopCachePayload()).toBeNull();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("reads nothing when the database cannot be opened", async () => {
    setIndexedDb(fakeIndexedDb({ openFails: true }));

    expect(await desktopCachePayload()).toBeNull();
  });

  it("reads nothing when the read request fails", async () => {
    const close = vi.fn();
    setIndexedDb(fakeIndexedDb({ close, read: () => fakeRequest((request) => request.onerror?.()) }));

    expect(await desktopCachePayload()).toBeNull();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("reads nothing where the page has no IndexedDB", async () => {
    delete (globalThis as { indexedDB?: unknown }).indexedDB;
    expect(await desktopCachePayload()).toBeNull();
  });
});
