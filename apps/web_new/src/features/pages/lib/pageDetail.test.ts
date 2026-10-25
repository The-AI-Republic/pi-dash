// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail model (parity/specs/pages.md §2.6, §2.7, §4, PAGE-022–025,
// 079–080): the two query keys and every address they compose, the
// sibling rows, the read/edit matrix and the loading/panel decision.

import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import {
  closeVersionSearch,
  filterSiblingOptions,
  historySearch,
  pageCanonicalHref,
  pageCapabilities,
  pageDetailDeepHref,
  pageDocumentTitle,
  pageSiblingOptions,
  pageStanding,
  PAGE_PANE_TABS,
  paneSearch,
  paneTabLabel,
  parsePageDetailSearch,
  resolvePageDetailBody,
  siblingHref,
  versionSearch,
} from "./pageDetail.js";
import { makePageRow } from "../testing.js";

const WS = "acme";
const PROJECT = "project-1";
const PAGE = "page-1";

describe("parsePageDetailSearch", () => {
  it("accepts each supported pane tab and a version id", () => {
    expect(parsePageDetailSearch({ paneTab: "outline" })).toEqual({ paneTab: "outline" });
    expect(parsePageDetailSearch({ paneTab: "info" })).toEqual({ paneTab: "info" });
    expect(parsePageDetailSearch({ paneTab: "assets" })).toEqual({ paneTab: "assets" });
    expect(parsePageDetailSearch({ version: "v1" })).toEqual({ version: "v1" });
  });

  it("drops unknown panes, blank versions and unrelated keys", () => {
    expect(parsePageDetailSearch({ paneTab: "bogus", type: "private" })).toEqual({});
    expect(parsePageDetailSearch({ version: "" })).toEqual({});
    expect(parsePageDetailSearch({ version: "   " })).toEqual({});
    expect(parsePageDetailSearch({ version: 42 })).toEqual({});
  });

  it("takes the first value of a repeated key", () => {
    expect(parsePageDetailSearch({ paneTab: ["info", "assets"], version: ["v2", "v3"] })).toEqual({
      paneTab: "info",
      version: "v2",
    });
  });

  it("ignores a repeated key whose first entry is not a string", () => {
    expect(parsePageDetailSearch({ paneTab: [1, "info"] })).toEqual({});
    expect(parsePageDetailSearch({ version: [] })).toEqual({});
  });

  it("returns an empty address state for an empty record", () => {
    expect(parsePageDetailSearch({})).toEqual({});
  });
});

describe("detail addresses", () => {
  it("composes the canonical address with no query string", () => {
    expect(pageCanonicalHref(WS, PROJECT, PAGE)).toBe("/acme/projects/project-1/pages/page-1");
    expect(siblingHref(WS, PROJECT, PAGE)).toBe("/acme/projects/project-1/pages/page-1");
  });

  it("writes whichever keys are set, in a stable order", () => {
    expect(pageDetailDeepHref(WS, PROJECT, PAGE, { paneTab: "info" })).toBe(
      "/acme/projects/project-1/pages/page-1?paneTab=info"
    );
    expect(pageDetailDeepHref(WS, PROJECT, PAGE, { version: "v1" })).toBe(
      "/acme/projects/project-1/pages/page-1?version=v1"
    );
    expect(pageDetailDeepHref(WS, PROJECT, PAGE, { paneTab: "info", version: "v1" })).toBe(
      "/acme/projects/project-1/pages/page-1?paneTab=info&version=v1"
    );
    expect(pageDetailDeepHref(WS, PROJECT, PAGE, {})).toBe("/acme/projects/project-1/pages/page-1");
  });

  it("closing the snapshot keeps the pane and drops the version", () => {
    expect(closeVersionSearch({ paneTab: "info", version: "v1" })).toEqual({ paneTab: "info" });
    expect(closeVersionSearch({ version: "v1" })).toEqual({});
    expect(closeVersionSearch({ paneTab: "assets" })).toEqual({ paneTab: "assets" });
  });

  it("the history shortcut writes the info pane and keeps a snapshot key", () => {
    expect(historySearch({})).toEqual({ paneTab: "info" });
    expect(historySearch({ version: "v1" })).toEqual({ paneTab: "info", version: "v1" });
  });

  it("selecting a version writes info by default and keeps an open pane", () => {
    expect(versionSearch("v9", {})).toEqual({ paneTab: "info", version: "v9" });
    expect(versionSearch("v9", { paneTab: "assets" })).toEqual({ paneTab: "assets", version: "v9" });
  });

  it("picking a pane tab keeps a snapshot; closing the pane drops only its key", () => {
    expect(paneSearch({}, "outline")).toEqual({ paneTab: "outline" });
    expect(paneSearch({ version: "v1" }, "assets")).toEqual({ paneTab: "assets", version: "v1" });
    expect(paneSearch({ paneTab: "info", version: "v1" }, null)).toEqual({ version: "v1" });
    expect(paneSearch({ paneTab: "info" }, null)).toEqual({});
  });

  it("names each pane tab and reads the unknown ones as none", () => {
    expect(PAGE_PANE_TABS.map((tab) => tab.label)).toEqual(["Outline", "Info", "Assets"]);
    expect(paneTabLabel("outline")).toBe("Outline");
    expect(paneTabLabel("assets")).toBe("Assets");
    expect(paneTabLabel(undefined)).toBe(null);
  });
});

describe("pageDocumentTitle", () => {
  it("follows the name and falls back for blanks", () => {
    expect(pageDocumentTitle("Launch notes")).toBe("Launch notes");
    expect(pageDocumentTitle("   ")).toBe("Untitled");
    expect(pageDocumentTitle("")).toBe("Untitled");
  });
});

describe("sibling options", () => {
  it("keeps live pages, names blanks and sorts by visible label", () => {
    const rows = [
      makePageRow({ id: "b", name: "Beta" }),
      makePageRow({ id: "a", name: "  " }),
      makePageRow({ id: "c", name: "Alpha" }),
      makePageRow({ id: "d", name: "Gone", archived_at: "2026-10-01T00:00:00Z" }),
    ];
    const options = pageSiblingOptions(rows);
    expect(options.map((option) => ({ id: option.id, name: option.name }))).toEqual([
      { id: "c", name: "Alpha" },
      { id: "b", name: "Beta" },
      { id: "a", name: "Untitled" },
    ]);
  });

  it("filters case-insensitively and returns everything for a blank query", () => {
    const options = [
      { id: "a", name: "Alpha", logoProps: {} },
      { id: "b", name: "Beta", logoProps: {} },
    ];
    expect(filterSiblingOptions(options, "")).toEqual(options);
    expect(filterSiblingOptions(options, "  ")).toEqual(options);
    expect(filterSiblingOptions(options, "BET")).toEqual([{ id: "b", name: "Beta", logoProps: {} }]);
    expect(filterSiblingOptions(options, "zzz")).toEqual([]);
  });
});

describe("pageStanding", () => {
  it("prefers ownership, then the stronger project role", () => {
    expect(pageStanding({ ownedBy: "u1", viewerId: "u1", projectRole: null })).toBe("owner");
    expect(pageStanding({ ownedBy: "u1", viewerId: "u2", projectRole: ROLE_ADMIN })).toBe("admin");
    expect(pageStanding({ ownedBy: "u1", viewerId: "u2", projectRole: ROLE_MEMBER })).toBe("member");
    expect(pageStanding({ ownedBy: "u1", viewerId: "u2", projectRole: ROLE_GUEST })).toBe("guest");
    expect(pageStanding({ ownedBy: "u1", viewerId: "u2", projectRole: 0 })).toBe("outsider");
    expect(pageStanding({ ownedBy: "u1", viewerId: "u2", projectRole: null })).toBe("outsider");
    expect(pageStanding({ ownedBy: "u1", viewerId: null, projectRole: ROLE_MEMBER })).toBe("member");
  });
});

describe("pageCapabilities", () => {
  const base = {
    ownedBy: "owner-id",
    access: 0,
    archivedAt: null,
    isLocked: false,
    viewerId: "viewer-id",
    projectRole: ROLE_MEMBER as number | null,
  };

  it("reads open pages for members and admins and edits them when unlocked", () => {
    for (const role of [ROLE_MEMBER, ROLE_ADMIN]) {
      const caps = pageCapabilities({ ...base, projectRole: role });
      expect(caps.canRead).toBe(true);
      expect(caps.canEdit).toBe(true);
      expect(caps.readOnly).toBe(false);
    }
    const owner = pageCapabilities({ ...base, viewerId: "owner-id", projectRole: ROLE_GUEST });
    expect(owner.canEdit).toBe(true);
    expect(owner.canManage).toBe(true);
  });

  it("never opens a restricted page for a non-owner, admin included", () => {
    for (const role of [ROLE_MEMBER, ROLE_ADMIN, ROLE_GUEST, null]) {
      const caps = pageCapabilities({ ...base, access: 1, projectRole: role });
      expect(caps.canRead).toBe(false);
      expect(caps.canEdit).toBe(false);
      expect(caps.readOnly).toBe(false);
    }
    const owner = pageCapabilities({ ...base, access: 1, viewerId: "owner-id" });
    expect(owner.canRead).toBe(true);
    expect(owner.canEdit).toBe(true);
  });

  it("panels a guest on an open page it does not own", () => {
    const caps = pageCapabilities({ ...base, projectRole: ROLE_GUEST });
    expect(caps.standing).toBe("guest");
    expect(caps.canRead).toBe(false);
    expect(caps.canEdit).toBe(false);
  });

  it("renders locked and shelved pages read-only, owner included", () => {
    for (const patch of [{ isLocked: true }, { archivedAt: "2026-10-01T00:00:00Z" }]) {
      const caps = pageCapabilities({ ...base, ...patch, viewerId: "owner-id", projectRole: ROLE_ADMIN });
      expect(caps.canRead).toBe(true);
      expect(caps.canEdit).toBe(false);
      expect(caps.readOnly).toBe(true);
    }
  });

  it("describes the two state flags it was given", () => {
    const caps = pageCapabilities({ ...base, isLocked: true, archivedAt: "2026-10-01T00:00:00Z" });
    expect(caps.isLocked).toBe(true);
    expect(caps.isShelved).toBe(true);
    expect(caps.canManage).toBe(false);
  });

  it("panels an outsider", () => {
    const caps = pageCapabilities({ ...base, projectRole: null, viewerId: null });
    expect(caps.standing).toBe("outsider");
    expect(caps.canRead).toBe(false);
  });
});

describe("resolvePageDetailBody", () => {
  it("spins until the record and identity settle", () => {
    expect(resolvePageDetailBody({ pending: true, recordError: false, canRead: false })).toBe("spinner");
    expect(resolvePageDetailBody({ pending: true, recordError: false, canRead: true })).toBe("spinner");
  });

  it("panels a failed or unreadable record and never flashes an editor", () => {
    expect(resolvePageDetailBody({ pending: false, recordError: true, canRead: true })).toBe("panel");
    expect(resolvePageDetailBody({ pending: false, recordError: false, canRead: false })).toBe("panel");
  });

  it("edits a settled, readable record", () => {
    expect(resolvePageDetailBody({ pending: false, recordError: false, canRead: true })).toBe("editor");
  });
});
