// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The per-project layout record (MOD-011, §13.3.1): one entry per project,
// unknown values dropped, corrupt records read as empty, a refused browser
// degrading to the in-memory choice, and the hook restoring on mount and
// writing through on change.

import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  MODULES_LIST_PREFS_STORAGE_KEY,
  parseLayoutRecord,
  readLayoutRecord,
  readProjectLayout,
  useModuleLayout,
  writeProjectLayout,
  type StorageLike,
} from "./listLayoutStore.js";

function memoryStorage(seed: Record<string, string> = {}): StorageLike & { data: Record<string, string> } {
  const data = { ...seed };
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => {
      data[key] = value;
    },
  };
}

beforeEach(() => {
  globalThis.localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("parseLayoutRecord", () => {
  it("reads a full record and drops unknown layouts and blank ids", () => {
    expect(parseLayoutRecord(JSON.stringify({ a: "board", b: "timeline", c: "bogus", "": "list" }))).toEqual({
      a: "board",
      b: "timeline",
      c: "list",
    });
  });

  it("reads empty for an absent, corrupt or non-object record", () => {
    expect(parseLayoutRecord(null)).toEqual({});
    expect(parseLayoutRecord("{not json")).toEqual({});
    expect(parseLayoutRecord("[1,2]")).toEqual({});
    expect(parseLayoutRecord("null")).toEqual({});
  });
});

describe("readLayoutRecord", () => {
  it("reads the stored record under the modules key", () => {
    const storage = memoryStorage({ [MODULES_LIST_PREFS_STORAGE_KEY]: JSON.stringify({ p1: "board" }) });
    expect(readLayoutRecord(storage)).toEqual({ p1: "board" });
  });

  it("reads empty without storage and when the browser throws", () => {
    expect(readLayoutRecord(undefined)).toEqual({});
    const hostile: StorageLike = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => undefined,
    };
    expect(readLayoutRecord(hostile)).toEqual({});
  });
});

describe("readProjectLayout", () => {
  it("returns the project's choice, else the default", () => {
    const storage = memoryStorage({ [MODULES_LIST_PREFS_STORAGE_KEY]: JSON.stringify({ p1: "timeline" }) });
    expect(readProjectLayout("p1", storage)).toBe("timeline");
    expect(readProjectLayout("p2", storage)).toBe("list");
  });
});

describe("writeProjectLayout", () => {
  it("keeps every other project's entry", () => {
    const storage = memoryStorage({ [MODULES_LIST_PREFS_STORAGE_KEY]: JSON.stringify({ p1: "board" }) });
    writeProjectLayout("p2", "timeline", storage);
    expect(JSON.parse(storage.data[MODULES_LIST_PREFS_STORAGE_KEY] ?? "{}")).toEqual({
      p1: "board",
      p2: "timeline",
    });
  });

  it("does nothing without storage or with a blank project id", () => {
    expect(() => writeProjectLayout("p1", "board", undefined)).not.toThrow();
    const storage = memoryStorage();
    writeProjectLayout("", "board", storage);
    expect(storage.data).toEqual({});
  });

  it("swallows a refused write", () => {
    const refused: StorageLike = {
      getItem: () => null,
      setItem: () => {
        throw new Error("quota");
      },
    };
    expect(() => writeProjectLayout("p1", "board", refused)).not.toThrow();
  });

  it("writes through the real browser record by default", () => {
    writeProjectLayout("p1", "timeline");
    expect(JSON.parse(globalThis.localStorage.getItem(MODULES_LIST_PREFS_STORAGE_KEY) ?? "{}")).toEqual({
      p1: "timeline",
    });
  });
});

function LayoutProbe({ projectId }: { projectId: string }): React.ReactElement {
  const { layout, setLayout } = useModuleLayout(projectId);
  return (
    <div>
      <span data-testid="layout">{layout}</span>
      <button type="button" onClick={() => setLayout("timeline")}>
        timeline
      </button>
    </div>
  );
}

describe("useModuleLayout", () => {
  it("restores the stored choice and writes a new one through", () => {
    writeProjectLayout("p1", "board");
    render(<LayoutProbe projectId="p1" />);
    expect(screen.getByTestId("layout")).toHaveTextContent("board");
    act(() => {
      screen.getByRole("button", { name: "timeline" }).click();
    });
    expect(screen.getByTestId("layout")).toHaveTextContent("timeline");
    expect(readProjectLayout("p1")).toBe("timeline");
  });

  it("re-reads the record when the project changes", () => {
    writeProjectLayout("p1", "board");
    writeProjectLayout("p2", "timeline");
    const { rerender } = render(<LayoutProbe projectId="p1" />);
    expect(screen.getByTestId("layout")).toHaveTextContent("board");
    rerender(<LayoutProbe projectId="p2" />);
    expect(screen.getByTestId("layout")).toHaveTextContent("timeline");
  });
});
