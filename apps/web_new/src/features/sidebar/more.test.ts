// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/permissions.js";
import {
  MORE_SECTION_OPEN_KEY,
  isMoreEntryActive,
  isMoreRoute,
  loadMoreOpen,
  moreHref,
  persistMoreOpen,
  selectMoreEntries,
} from "./more.js";

describe("moreHref", () => {
  it("addresses the destination below the workspace slug", () => {
    expect(moreHref("acme", "schedulers")).toBe("/acme/schedulers/");
    expect(moreHref("acme", "projects/archives")).toBe("/acme/projects/archives/");
  });
});

describe("selectMoreEntries", () => {
  it("lists every destination for members, in catalogue order", () => {
    const entries = selectMoreEntries({ workspaceSlug: "acme", role: ROLE_MEMBER });
    expect(entries.map((entry) => entry.name)).toEqual([
      "Projects",
      "AI Workers",
      "Prompts",
      "Schedulers",
      "AI Dev Machines",
      "Analytics",
      "Archives",
      "Active cycles",
    ]);
    expect(entries[3]).toMatchObject({ key: "schedulers", href: "/acme/schedulers/" });
    expect(entries[6]).toMatchObject({ key: "archives", href: "/acme/projects/archives/" });
    expect(entries[7]).toMatchObject({ key: "active-cycles", href: "/acme/active-cycles/" });
  });

  it("lists every destination for admins", () => {
    expect(selectMoreEntries({ workspaceSlug: "acme", role: ROLE_ADMIN })).toHaveLength(8);
  });

  it("shows guests the shared rows only", () => {
    const entries = selectMoreEntries({ workspaceSlug: "acme", role: ROLE_GUEST });
    expect(entries.map((entry) => entry.name)).toEqual(["Projects", "Prompts", "Schedulers"]);
  });

  it("reads a null role as guest level", () => {
    const entries = selectMoreEntries({ workspaceSlug: "acme", role: null });
    expect(entries.map((entry) => entry.name)).toEqual(["Projects", "Prompts", "Schedulers"]);
  });
});

describe("isMoreEntryActive", () => {
  it("matches the destination address with or without trailing slashes", () => {
    expect(isMoreEntryActive("/acme/schedulers/", "/acme/schedulers/")).toBe(true);
    expect(isMoreEntryActive("/acme/schedulers", "/acme/schedulers/")).toBe(true);
    expect(isMoreEntryActive("/acme/schedulers/", "/acme/schedulers")).toBe(true);
  });

  it("matches nested addresses under the destination", () => {
    expect(isMoreEntryActive("/acme/schedulers/nightly/", "/acme/schedulers/")).toBe(true);
  });

  it("rejects sibling addresses sharing a prefix", () => {
    expect(isMoreEntryActive("/acme/schedulers-archive/", "/acme/schedulers/")).toBe(false);
    expect(isMoreEntryActive("/acme/", "/acme/schedulers/")).toBe(false);
  });
});

describe("isMoreRoute", () => {
  it("matches hosted member routes", () => {
    expect(isMoreRoute("/acme/schedulers/", "acme")).toBe(true);
    expect(isMoreRoute("/acme/analytics/", "acme")).toBe(true);
    expect(isMoreRoute("/acme/runners/abc/", "acme")).toBe(true);
    expect(isMoreRoute("/acme/projects/archives/", "acme")).toBe(true);
    expect(isMoreRoute("/acme/active-cycles/", "acme")).toBe(true);
  });

  it("ignores the projects list and project pages", () => {
    expect(isMoreRoute("/acme/projects/", "acme")).toBe(false);
    expect(isMoreRoute("/acme/projects/p1/issues/", "acme")).toBe(false);
  });

  it("ignores project archive pages that merely contain the segment", () => {
    expect(isMoreRoute("/acme/projects/p1/archives/issues/", "acme")).toBe(false);
  });

  it("ignores unrelated workspace routes", () => {
    expect(isMoreRoute("/acme/", "acme")).toBe(false);
    expect(isMoreRoute("/acme/drafts/", "acme")).toBe(false);
  });
});

describe("more open flag", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("starts closed and round-trips the persisted flag", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
      removeItem: (key: string) => void store.delete(key),
    });
    expect(loadMoreOpen()).toBe(false);
    persistMoreOpen(true);
    expect(store.get(MORE_SECTION_OPEN_KEY)).toBe("1");
    expect(loadMoreOpen()).toBe(true);
    persistMoreOpen(false);
    expect(store.has(MORE_SECTION_OPEN_KEY)).toBe(false);
    expect(loadMoreOpen()).toBe(false);
  });

  it("reads closed when storage is missing or denied", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(loadMoreOpen()).toBe(false);
    expect(() => persistMoreOpen(true)).not.toThrow();
    const denied = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
      removeItem: () => {
        throw new Error("denied");
      },
    };
    vi.stubGlobal("localStorage", denied);
    expect(loadMoreOpen()).toBe(false);
    expect(() => persistMoreOpen(true)).not.toThrow();
  });
});
