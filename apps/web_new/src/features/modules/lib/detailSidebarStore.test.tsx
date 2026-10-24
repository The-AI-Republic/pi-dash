// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail sidebar collapse record (MOD-032): absent and corrupt
// entries read as expanded, writes persist a JSON boolean, a refused
// browser degrades to the in-memory choice, and the hook restores on
// mount and writes through on change.

import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  MODULE_DETAIL_SIDEBAR_STORAGE_KEY,
  parseSidebarCollapsed,
  readSidebarCollapsed,
  useDetailSidebarCollapsed,
  writeSidebarCollapsed,
} from "./detailSidebarStore.js";
import type { StorageLike } from "./listLayoutStore.js";

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

describe("parseSidebarCollapsed", () => {
  it("reads true only for a stored JSON true", () => {
    expect(parseSidebarCollapsed("true")).toBe(true);
    expect(parseSidebarCollapsed("false")).toBe(false);
    expect(parseSidebarCollapsed(null)).toBe(false);
    expect(parseSidebarCollapsed("")).toBe(false);
    expect(parseSidebarCollapsed("yes")).toBe(false);
    expect(parseSidebarCollapsed("1")).toBe(false);
  });
});

describe("readSidebarCollapsed", () => {
  it("reads the stored flag", () => {
    const storage = memoryStorage({ [MODULE_DETAIL_SIDEBAR_STORAGE_KEY]: "true" });
    expect(readSidebarCollapsed(storage)).toBe(true);
    expect(readSidebarCollapsed(memoryStorage())).toBe(false);
  });

  it("reads expanded without storage or on a refused read", () => {
    expect(readSidebarCollapsed(undefined)).toBe(false);
    const refused: StorageLike = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {},
    };
    expect(readSidebarCollapsed(refused)).toBe(false);
  });

  it("writes through the real browser record by default", () => {
    writeSidebarCollapsed(true);
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("true");
    writeSidebarCollapsed(false);
    expect(globalThis.localStorage.getItem(MODULE_DETAIL_SIDEBAR_STORAGE_KEY)).toBe("false");
  });

  it("ignores a refused write", () => {
    const refused: StorageLike = {
      getItem: () => null,
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(() => writeSidebarCollapsed(true, refused)).not.toThrow();
    writeSidebarCollapsed(true, undefined);
  });
});

function CollapseProbe(): React.ReactElement {
  const { collapsed, setCollapsed } = useDetailSidebarCollapsed();
  return (
    <div>
      <span data-testid="collapsed">{String(collapsed)}</span>
      <button type="button" onClick={() => setCollapsed(true)}>
        collapse
      </button>
      <button type="button" onClick={() => setCollapsed(false)}>
        expand
      </button>
    </div>
  );
}

describe("useDetailSidebarCollapsed", () => {
  it("restores the stored choice and writes a new one through", () => {
    writeSidebarCollapsed(true);
    render(<CollapseProbe />);
    expect(screen.getByTestId("collapsed")).toHaveTextContent("true");
    act(() => {
      screen.getByRole("button", { name: "expand" }).click();
    });
    expect(screen.getByTestId("collapsed")).toHaveTextContent("false");
    expect(readSidebarCollapsed()).toBe(false);
  });

  it("starts expanded with no stored choice", () => {
    render(<CollapseProbe />);
    expect(screen.getByTestId("collapsed")).toHaveTextContent("false");
    act(() => {
      screen.getByRole("button", { name: "collapse" }).click();
    });
    expect(screen.getByTestId("collapsed")).toHaveTextContent("true");
    expect(readSidebarCollapsed()).toBe(true);
  });
});
