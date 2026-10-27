// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page state over the collaboration channel (parity/specs/pages.md §2.3,
// §3.8, PAGE-058): local lock events map to bare wire names, inbound
// payloads parse to property events only when they are bare client
// names, sends swallow exactly one echo each in order, and applying a
// peer event patches the cached rows without another read.

import { QueryClient } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";

import { pageKeys } from "../api/keys.js";
import { PAGE_EVENT_LOCKED, PAGE_EVENT_UNLOCKED } from "../api/rowMutations.js";
import { makePageRow } from "../testing.js";
import {
  applyPageLiveProperty,
  clearPageLivePropertyEchoes,
  notePageLivePropertySent,
  pageLivePropertyWire,
  parsePageLiveProperty,
  resetPageLivePropertyEchoes,
  takePageLivePropertyEcho,
} from "./pageLivePropertyEvents.js";

const SLUG = "acme";
const PROJECT = "project-1";
const PAGE = "323e4567-e89b-12d3-a456-426614174002";

afterEach(() => {
  resetPageLivePropertyEchoes();
});

describe("pageLivePropertyWire", () => {
  it("maps the lock pair to bare wire names", () => {
    expect(pageLivePropertyWire(PAGE_EVENT_LOCKED)).toBe("lock");
    expect(pageLivePropertyWire(PAGE_EVENT_UNLOCKED)).toBe("unlock");
  });
});

describe("parsePageLiveProperty", () => {
  it("reads bare client names and rejects every frame shape", () => {
    expect(parsePageLiveProperty(PAGE_EVENT_LOCKED)).toBe("locked");
    expect(parsePageLiveProperty(PAGE_EVENT_UNLOCKED)).toBe("unlocked");
    expect(parsePageLiveProperty('{"action":"error"}')).toBeNull();
    expect(parsePageLiveProperty("archived")).toBeNull();
    expect(parsePageLiveProperty("lock")).toBeNull();
    expect(parsePageLiveProperty("Lock")).toBeNull();
    expect(parsePageLiveProperty("")).toBeNull();
  });
});

describe("property echoes", () => {
  it("swallows one echo per send, oldest first", () => {
    notePageLivePropertySent(PAGE, PAGE_EVENT_LOCKED);
    notePageLivePropertySent(PAGE, PAGE_EVENT_UNLOCKED);

    expect(takePageLivePropertyEcho(PAGE, PAGE_EVENT_LOCKED)).toBe(true);
    expect(takePageLivePropertyEcho(PAGE, PAGE_EVENT_UNLOCKED)).toBe(true);
    expect(takePageLivePropertyEcho(PAGE, PAGE_EVENT_UNLOCKED)).toBe(false);
  });

  it("leaves genuine peer events and other pages alone", () => {
    notePageLivePropertySent(PAGE, PAGE_EVENT_LOCKED);

    expect(takePageLivePropertyEcho(PAGE, PAGE_EVENT_UNLOCKED)).toBe(false);
    expect(takePageLivePropertyEcho("other-page", PAGE_EVENT_LOCKED)).toBe(false);
    expect(takePageLivePropertyEcho(PAGE, PAGE_EVENT_LOCKED)).toBe(true);
  });

  it("clears one page without touching the others", () => {
    notePageLivePropertySent(PAGE, PAGE_EVENT_LOCKED);
    notePageLivePropertySent("other-page", PAGE_EVENT_LOCKED);

    clearPageLivePropertyEchoes(PAGE);

    expect(takePageLivePropertyEcho(PAGE, PAGE_EVENT_LOCKED)).toBe(false);
    expect(takePageLivePropertyEcho("other-page", PAGE_EVENT_LOCKED)).toBe(true);
  });

  it("drops the oldest send past its cap instead of growing forever", () => {
    for (let index = 0; index < 17; index += 1) notePageLivePropertySent(PAGE, PAGE_EVENT_LOCKED);

    let swallowed = 0;
    while (takePageLivePropertyEcho(PAGE, PAGE_EVENT_LOCKED)) swallowed += 1;
    expect(swallowed).toBe(16);
  });
});

describe("applyPageLiveProperty", () => {
  function seeded(): QueryClient {
    const client = new QueryClient();
    client.setQueryData(pageKeys.detail(SLUG, PROJECT, PAGE), makePageRow({ id: PAGE, is_locked: false }));
    client.setQueryData(pageKeys.list(SLUG, PROJECT), [
      makePageRow({ id: PAGE, is_locked: false }),
      makePageRow({ id: "other-page", is_locked: false }),
    ]);
    return client;
  }

  function read(client: QueryClient): { detail: boolean | undefined; rows: Array<{ id: string; locked: boolean }> } {
    const detail = client.getQueryData<ReturnType<typeof makePageRow>>(pageKeys.detail(SLUG, PROJECT, PAGE));
    const rows = client.getQueryData<ReturnType<typeof makePageRow>[]>(pageKeys.list(SLUG, PROJECT)) ?? [];
    return { detail: detail?.is_locked, rows: rows.map((row) => ({ id: row.id, locked: row.is_locked })) };
  }

  it("freezes and frees both cached rows with no other read", () => {
    const client = seeded();

    applyPageLiveProperty(client, pageKeys.detail(SLUG, PROJECT, PAGE), pageKeys.list(SLUG, PROJECT), PAGE, "locked");
    expect(read(client)).toEqual({
      detail: true,
      rows: [
        { id: PAGE, locked: true },
        { id: "other-page", locked: false },
      ],
    });

    applyPageLiveProperty(client, pageKeys.detail(SLUG, PROJECT, PAGE), pageKeys.list(SLUG, PROJECT), PAGE, "unlocked");
    expect(read(client)).toEqual({
      detail: false,
      rows: [
        { id: PAGE, locked: false },
        { id: "other-page", locked: false },
      ],
    });
  });

  it("keeps other rows identical and absent caches absent", () => {
    const client = seeded();
    const before = client.getQueryData<ReturnType<typeof makePageRow>[]>(pageKeys.list(SLUG, PROJECT));
    const otherBefore = before?.find((row) => row.id === "other-page");

    applyPageLiveProperty(client, pageKeys.detail(SLUG, PROJECT, PAGE), pageKeys.list(SLUG, PROJECT), PAGE, "locked");

    const after = client.getQueryData<ReturnType<typeof makePageRow>[]>(pageKeys.list(SLUG, PROJECT));
    expect(after?.find((row) => row.id === "other-page")).toBe(otherBefore);

    const empty = new QueryClient();
    applyPageLiveProperty(empty, pageKeys.detail(SLUG, PROJECT, PAGE), pageKeys.list(SLUG, PROJECT), PAGE, "locked");
    expect(empty.getQueryData(pageKeys.detail(SLUG, PROJECT, PAGE))).toBeUndefined();
    expect(empty.getQueryData(pageKeys.list(SLUG, PROJECT))).toBeUndefined();
  });

  it("ignores detail rows that name another page", () => {
    const client = new QueryClient();
    const stranger = makePageRow({ id: "other-page", is_locked: false });
    client.setQueryData(pageKeys.detail(SLUG, PROJECT, PAGE), stranger);

    applyPageLiveProperty(client, pageKeys.detail(SLUG, PROJECT, PAGE), pageKeys.list(SLUG, PROJECT), PAGE, "locked");

    expect(client.getQueryData(pageKeys.detail(SLUG, PROJECT, PAGE))).toBe(stranger);
  });
});
