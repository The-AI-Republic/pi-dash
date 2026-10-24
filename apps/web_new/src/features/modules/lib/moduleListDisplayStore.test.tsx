// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The per-project modules list display record (§13.3.1, MOD-004/005/006):
// one entry per project, each entry validated so corrupt fields fall back
// to the documented defaults, unknown keys dropped, writes keeping other
// projects, and the live hook following every write.

import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { StorageLike } from "./listLayoutStore.js";
import {
  MODULES_LIST_DISPLAY_STORAGE_KEY,
  parseDisplayRecord,
  readDisplayRecord,
  readProjectDisplay,
  resetModuleDisplayCache,
  useModuleListDisplay,
  writeProjectDisplay,
} from "./moduleListDisplayStore.js";

function memoryStorage(
  seed: Record<string, string> = {},
  refuseWrites = false
): StorageLike & { data: Record<string, string> } {
  const data = { ...seed };
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => {
      if (refuseWrites) throw new Error("quota");
      data[key] = value;
    },
  };
}

const DISPLAY = {
  query: "alpha",
  sort: { key: "due-date", direction: "descending" },
  filters: {
    favorites: true,
    status: ["completed"],
    lead: ["u1"],
    members: [],
    "start-date": [{ kind: "preset", value: "week-1" }],
    "due-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
  },
} as const;

beforeEach(() => {
  globalThis.localStorage.clear();
  resetModuleDisplayCache();
});

afterEach(() => {
  resetModuleDisplayCache();
});

describe("parseDisplayRecord", () => {
  it("reads a valid entry and drops blank ids", () => {
    const record = parseDisplayRecord(JSON.stringify({ p1: DISPLAY, "": DISPLAY }));
    expect(Object.keys(record)).toEqual(["p1"]);
    expect(record.p1).toEqual({
      query: "alpha",
      sort: { key: "due-date", direction: "descending" },
      filters: {
        favorites: true,
        status: ["completed"],
        lead: ["u1"],
        members: [],
        "start-date": [{ kind: "preset", value: "week-1" }],
        "due-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
      },
    });
  });

  it("drops invalid and unknown entries rather than guessing", () => {
    const bad = [
      { ...DISPLAY, sort: { key: "mystery", direction: "ascending" } },
      { ...DISPLAY, sort: { key: "name", direction: "sideways" } },
      { ...DISPLAY, filters: { ...DISPLAY.filters, status: ["mystery"] } },
      { ...DISPLAY, filters: { ...DISPLAY.filters, "start-date": [{ kind: "preset", value: "year-9" }] } },
      { ...DISPLAY, filters: { ...DISPLAY.filters, "due-date": [{ kind: "range", value: { start: "x", end: "y" } }] } },
      { ...DISPLAY, query: 7 },
      "not an entry",
      null,
    ];
    const raw = Object.fromEntries(bad.map((value, index) => [`p${index}`, value]));
    expect(parseDisplayRecord(JSON.stringify({ ...raw, good: DISPLAY }))).toHaveProperty("good");
    expect(Object.keys(parseDisplayRecord(JSON.stringify(raw)))).toEqual([]);
  });

  it("reads empty for an absent, corrupt or non-object record", () => {
    expect(parseDisplayRecord(null)).toEqual({});
    expect(parseDisplayRecord("{not json")).toEqual({});
    expect(parseDisplayRecord("[1,2]")).toEqual({});
    expect(parseDisplayRecord("null")).toEqual({});
  });

  it("keeps the documented defaults when no record is stored", () => {
    const storage = memoryStorage();
    expect(readDisplayRecord(storage)).toEqual({});
    expect(readProjectDisplay("p1", storage)).toEqual({
      query: "",
      sort: { key: "name", direction: "ascending" },
      filters: { favorites: false, status: [], lead: [], members: [], "start-date": [], "due-date": [] },
    });
  });

  it("reads empty when storage itself refuses or is absent", () => {
    const refusing: StorageLike = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => undefined,
    };
    expect(readDisplayRecord(refusing)).toEqual({});
    expect(readDisplayRecord(undefined)).toEqual({});
  });

  it("falls back to the defaults when the browser refuses storage access outright", () => {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      get() {
        throw new Error("blocked by policy");
      },
    });
    try {
      expect(readDisplayRecord()).toEqual({});
      expect(readProjectDisplay("p1").query).toBe("");
      // A refused write keeps this visit's choice without throwing.
      writeProjectDisplay("p1", { ...readProjectDisplay("p1"), query: "kept" });
      expect(readProjectDisplay("p1").query).toBe("");
    } finally {
      if (descriptor === undefined) delete (globalThis as { localStorage?: unknown }).localStorage;
      else Object.defineProperty(globalThis, "localStorage", descriptor);
    }
  });
});

describe("writeProjectDisplay", () => {
  it("keeps every other project's entry", () => {
    const storage = memoryStorage({ [MODULES_LIST_DISPLAY_STORAGE_KEY]: JSON.stringify({ other: DISPLAY }) });
    writeProjectDisplay("p1", readProjectDisplay("p1", storage), storage);
    const stored = readDisplayRecord(storage);
    expect(Object.keys(stored).sort()).toEqual(["other", "p1"]);
    expect(stored.other?.query).toBe("alpha");
  });

  it("ignores a blank project id and an absent storage", () => {
    const storage = memoryStorage();
    writeProjectDisplay("", readProjectDisplay("p1", storage), storage);
    writeProjectDisplay("p1", readProjectDisplay("p1"), undefined);
    expect(readDisplayRecord(storage)).toEqual({});
  });

  it("survives a refused write and remembers the choice for this visit", () => {
    const storage = memoryStorage({}, true);
    writeProjectDisplay("p1", { ...readProjectDisplay("p1", storage), query: "kept" }, storage);
    expect(readDisplayRecord(storage)).toEqual({});
    expect(readProjectDisplay("p1", storage).query).toBe("");
  });
});

function Probe({ storage }: { storage: StorageLike }): React.ReactElement {
  const { display, setQuery, setSort, setFilters } = useModuleListDisplay("p1", storage);
  return (
    <div>
      <span data-testid="query">{display.query}</span>
      <span data-testid="sort">{`${display.sort.key}:${display.sort.direction}`}</span>
      <span data-testid="status">{display.filters.status.join(",")}</span>
      <span data-testid="other">{readProjectDisplay("p2", storage).query}</span>
      <button onClick={() => setQuery("bravo")}>query</button>
      <button onClick={() => setSort({ key: "manual", direction: "descending" })}>sort</button>
      <button onClick={() => setFilters({ ...display.filters, status: ["planned"] })}>filters</button>
    </div>
  );
}

describe("useModuleListDisplay", () => {
  it("restores the stored entry, writes each setter through and leaves other projects alone", () => {
    const storage = memoryStorage({ [MODULES_LIST_DISPLAY_STORAGE_KEY]: JSON.stringify({ p1: DISPLAY }) });
    render(<Probe storage={storage} />);
    expect(screen.getByTestId("query")).toHaveTextContent("alpha");
    expect(screen.getByTestId("sort")).toHaveTextContent("due-date:descending");
    expect(screen.getByTestId("status")).toHaveTextContent("completed");

    act(() => screen.getByRole("button", { name: "query" }).click());
    expect(screen.getByTestId("query")).toHaveTextContent("bravo");
    expect(readProjectDisplay("p1", storage).query).toBe("bravo");

    act(() => screen.getByRole("button", { name: "sort" }).click());
    expect(screen.getByTestId("sort")).toHaveTextContent("manual:descending");

    act(() => screen.getByRole("button", { name: "filters" }).click());
    expect(screen.getByTestId("status")).toHaveTextContent("planned");
    expect(readProjectDisplay("p1", storage).filters.status).toEqual(["planned"]);
    expect(screen.getByTestId("other")).toHaveTextContent("");
  });

  it("starts from the documented defaults for a project with no entry", () => {
    render(<Probe storage={memoryStorage()} />);
    expect(screen.getByTestId("query")).toHaveTextContent("");
    expect(screen.getByTestId("sort")).toHaveTextContent("name:ascending");
    expect(screen.getByTestId("status")).toBeEmptyDOMElement();
  });
});
