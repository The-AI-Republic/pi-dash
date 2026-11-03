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
  ARCHIVED_MODULES_DISPLAY_STORAGE_KEY,
  archivedModulesDisplayScope,
  currentArchivedProjectDisplay,
  MODULES_LIST_DISPLAY_STORAGE_KEY,
  modulesListDisplayScope,
  parseDisplayRecord,
  readArchivedProjectDisplay,
  readDisplayRecord,
  readProjectDisplay,
  resetArchivedDisplayCache,
  resetModuleDisplayCache,
  useArchivedModuleDisplay,
  useModuleListDisplay,
  writeArchivedProjectDisplay,
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

const DISPLAY: ModuleListDisplayInput = {
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
};

type ModuleListDisplayInput = Parameters<typeof writeProjectDisplay>[1];

/** A mutable copy of DISPLAY's filters (the fixture itself is readonly). */
function displayFilters(): ModuleListDisplayInput["filters"] {
  return {
    favorites: true,
    status: ["completed"],
    lead: ["u1"],
    members: [],
    "start-date": [{ kind: "preset", value: "week-1" }],
    "due-date": [{ kind: "range", value: { start: "2026-10-20", end: "2026-11-20" } }],
  };
}

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

describe("archived screen scope", () => {
  it("names its own browser record, apart from the live list's", () => {
    expect(ARCHIVED_MODULES_DISPLAY_STORAGE_KEY).toBe("archived_modules_display");
    expect(ARCHIVED_MODULES_DISPLAY_STORAGE_KEY).not.toBe(MODULES_LIST_DISPLAY_STORAGE_KEY);
    expect(archivedModulesDisplayScope.storageKey).toBe(ARCHIVED_MODULES_DISPLAY_STORAGE_KEY);
  });

  it("writes and reads the archived record without touching the live one", () => {
    const storage = memoryStorage();
    writeProjectDisplay("p1", { ...readProjectDisplay("p1", storage), query: "live" }, storage);
    writeArchivedProjectDisplay(
      "p1",
      { ...readArchivedProjectDisplay("p1", storage), query: "archived", filters: displayFilters() },
      storage
    );
    // The archived scope keeps its filters and ordering, never its search
    // (archives §5.2), while the live list still remembers its own.
    expect(readArchivedProjectDisplay("p1", storage).query).toBe("");
    expect(readArchivedProjectDisplay("p1", storage).filters.lead).toEqual(["u1"]);
    expect(readProjectDisplay("p1", storage).query).toBe("live");
    expect(archivedModulesDisplayScope.readRecord(storage)).toHaveProperty("p1");
    expect(modulesListDisplayScope.readRecord(storage)).toHaveProperty("p1");
  });

  it("never writes the archived search, even when the writer passes one", () => {
    const storage = memoryStorage();
    writeArchivedProjectDisplay(
      "p1",
      { ...readArchivedProjectDisplay("p1", storage), filters: displayFilters(), query: "atlas" },
      storage
    );
    const raw = JSON.parse(storage.data[ARCHIVED_MODULES_DISPLAY_STORAGE_KEY] ?? "{}") as Record<
      string,
      { query?: string; filters?: { lead?: string[] } }
    >;
    expect(raw.p1?.query).toBe("");
    expect(raw.p1?.filters?.lead).toEqual(["u1"]);
  });

  it("gives the current project the remembered archived narrowing without its search", () => {
    const storage = memoryStorage({ [ARCHIVED_MODULES_DISPLAY_STORAGE_KEY]: JSON.stringify({ p1: DISPLAY }) });
    const current = currentArchivedProjectDisplay("p1", storage);
    // A record left by an older build can carry a query; the archived
    // scope ignores it so a reload always starts with an empty box.
    expect(current.query).toBe("");
    expect(current.sort).toEqual({ key: "due-date", direction: "descending" });
    expect(current.filters.lead).toEqual(["u1"]);
    // Repeat reads share the reference until the next write.
    expect(currentArchivedProjectDisplay("p1", storage)).toBe(current);
  });

  it("drops the archived search when the screen is entered again", () => {
    const storage = memoryStorage();
    function SearchProbe(): React.ReactElement {
      const { display, setQuery } = useArchivedModuleDisplay("p1", storage);
      return (
        <div>
          <span data-testid="session-query">{display.query}</span>
          <button onClick={() => setQuery("atlas")}>search</button>
        </div>
      );
    }
    render(<SearchProbe />);
    act(() => screen.getByRole("button", { name: "search" }).click());
    // This visit keeps what was typed...
    expect(screen.getByTestId("session-query")).toHaveTextContent("atlas");
    expect(readArchivedProjectDisplay("p1", storage).query).toBe("");
    // ...and a reload starts from the stored entry, which carries no search.
    resetArchivedDisplayCache();
    expect(currentArchivedProjectDisplay("p1", storage).query).toBe("");
  });

  it("keeps the live list's search in storage so it survives a reload", () => {
    const storage = memoryStorage();
    writeProjectDisplay(
      "p1",
      { ...readProjectDisplay("p1", storage), filters: displayFilters(), query: "alpha" },
      storage
    );
    const raw = JSON.parse(storage.data[MODULES_LIST_DISPLAY_STORAGE_KEY] ?? "{}") as Record<
      string,
      { query?: string }
    >;
    expect(raw.p1?.query).toBe("alpha");
  });

  it("survives a refused archived write without inventing a stored entry", () => {
    const storage = memoryStorage({}, true);
    writeArchivedProjectDisplay(
      "p1",
      { ...readArchivedProjectDisplay("p1", storage), filters: displayFilters() },
      storage
    );
    expect(archivedModulesDisplayScope.readRecord(storage)).toEqual({});
    expect(readArchivedProjectDisplay("p1", storage).filters.lead).toEqual([]);
  });

  it("defaults an unknown project to an unnarrowed archive", () => {
    const current = currentArchivedProjectDisplay("p9", memoryStorage());
    expect(current.query).toBe("");
    expect(current.sort).toEqual({ key: "name", direction: "ascending" });
    expect(current.filters).toEqual({
      favorites: false,
      status: [],
      lead: [],
      members: [],
      "start-date": [],
      "due-date": [],
    });
  });

  it("drops cached values and subscribers when reset", () => {
    const storage = memoryStorage();
    const before = currentArchivedProjectDisplay("p1", storage);
    resetArchivedDisplayCache();
    expect(currentArchivedProjectDisplay("p1", storage)).not.toBe(before);
  });

  it("follows its own writes and never notifies the live list's hook", () => {
    const storage = memoryStorage();
    function ArchivedProbe(): React.ReactElement {
      const { display, setQuery } = useArchivedModuleDisplay("p1", storage);
      return (
        <div>
          <span data-testid="archived-query">{display.query}</span>
          <span data-testid="archived-lead">{display.filters.lead.join(",")}</span>
          <button onClick={() => setQuery("atlas")}>set-query</button>
        </div>
      );
    }
    function useArchivedWithFilters(): ReturnType<typeof useArchivedModuleDisplay> {
      return useArchivedModuleDisplay("p1", storage);
    }
    function ArchivedFiltersProbe(): React.ReactElement {
      const { setFilters, display } = useArchivedWithFilters();
      return <button onClick={() => setFilters({ ...display.filters, lead: ["u1"] })}>set-lead</button>;
    }
    function LiveProbe(): React.ReactElement {
      return <span data-testid="live-query">{useModuleListDisplay("p1", storage).display.query}</span>;
    }
    render(
      <>
        <ArchivedProbe />
        <ArchivedFiltersProbe />
        <LiveProbe />
      </>
    );
    act(() => screen.getByRole("button", { name: "set-query" }).click());
    expect(screen.getByTestId("archived-query")).toHaveTextContent("atlas");
    expect(screen.getByTestId("live-query")).toHaveTextContent("");
    // In memory only: the stored entry never gains the search text.
    expect(readArchivedProjectDisplay("p1", storage).query).toBe("");
    expect(readProjectDisplay("p1", storage).query).toBe("");
    act(() => screen.getByRole("button", { name: "set-lead" }).click());
    expect(screen.getByTestId("archived-lead")).toHaveTextContent("u1");
    expect(readArchivedProjectDisplay("p1", storage).filters.lead).toEqual(["u1"]);
    // A later narrowing write never drops the session's own search.
    expect(screen.getByTestId("archived-query")).toHaveTextContent("atlas");
  });
});
