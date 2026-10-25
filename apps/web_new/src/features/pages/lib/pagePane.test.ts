// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pane model (parity/specs/pages.md §6.5–§6.9, PAGE-045–050): outline,
// statistics, asset rows, attribution and the preview bars, all derived
// from one document state.

import { describe, expect, it } from "vitest";

import {
  assetFileName,
  EMPTY_PAGE_DOCUMENT,
  PAGE_DEACTIVATED_USER,
  PAGE_PANE_TAB_ORDER,
  PAGE_PANE_WIDTH_PX,
  PAGE_PREVIEW_BAR_WIDTH_PX,
  PAGE_OUTLINE_INDENT_PX,
  pageActorRows,
  pageAssetDownloadHref,
  pageAssetViewHref,
  pageActorMembers,
  pageCalendarDate,
  pageMemberName,
  pageMemberProfileHref,
  pagePreviewBars,
  pageReadTimeLabel,
  pageReadTimeSeconds,
  parsePageDocument,
  shortRelativeAge,
} from "./pagePane.js";

const SCOPE = { workspaceSlug: "acme", projectId: "project-1" };

describe("tabs and geometry", () => {
  it("orders the tabs Outline, Info, Assets", () => {
    expect(PAGE_PANE_TAB_ORDER).toEqual(["outline", "info", "assets"]);
  });

  it("pins a fixed pane width inside the oracle's range", () => {
    expect(PAGE_PANE_WIDTH_PX).toBeGreaterThan(290);
    expect(PAGE_PANE_WIDTH_PX).toBeLessThan(298);
  });

  it("indents outline levels 1:2:4 and narrows preview bars by level", () => {
    expect(PAGE_OUTLINE_INDENT_PX[2] / PAGE_OUTLINE_INDENT_PX[1]).toBe(2);
    expect(PAGE_OUTLINE_INDENT_PX[3] / PAGE_OUTLINE_INDENT_PX[1]).toBe(4);
    expect(PAGE_PREVIEW_BAR_WIDTH_PX[1]).toBeGreaterThan(PAGE_PREVIEW_BAR_WIDTH_PX[2]);
    expect(PAGE_PREVIEW_BAR_WIDTH_PX[2]).toBeGreaterThan(PAGE_PREVIEW_BAR_WIDTH_PX[3]);
  });
});

describe("parsePageDocument", () => {
  it("reads an empty document as empty", () => {
    expect(parsePageDocument("", SCOPE)).toEqual(EMPTY_PAGE_DOCUMENT);
    expect(parsePageDocument("<p></p>", SCOPE)).toEqual(EMPTY_PAGE_DOCUMENT);
  });

  it("lists first- through third-level headings in order with per-level ranks", () => {
    const doc = parsePageDocument(
      "<h1>Top</h1><p>a</p><h2>Mid</h2><h3>Low</h3><h2>Mid two</h2><h1>Top two</h1>",
      SCOPE
    );
    expect(doc.headings).toEqual([
      { level: 1, text: "Top", levelIndex: 1 },
      { level: 2, text: "Mid", levelIndex: 1 },
      { level: 3, text: "Low", levelIndex: 1 },
      { level: 2, text: "Mid two", levelIndex: 2 },
      { level: 1, text: "Top two", levelIndex: 2 },
    ]);
  });

  it("keeps only the first three heading levels, but counts their text", () => {
    const doc = parsePageDocument("<h4>Deep</h4><p>body words</p>", SCOPE);
    expect(doc.headings).toEqual([]);
    expect(doc.stats.words).toBe(3);
  });

  it("counts words, characters and prose paragraphs once per innermost block", () => {
    const doc = parsePageDocument(
      "<h1>Title here</h1><p>Two words</p><ul><li>one</li><li>two</li></ul><blockquote><p>quoted</p></blockquote>",
      SCOPE
    );
    // Words span every block; paragraphs only count prose paragraphs.
    expect(doc.stats.words).toBe(2 + 2 + 1 + 1 + 1);
    expect(doc.stats.paragraphs).toBe(2);
    expect(doc.stats.characters).toBe(
      "Title here".length + "Two words".length + "one".length + "two".length + "quoted".length
    );
  });

  it("collapses runs of whitespace before counting", () => {
    expect(parsePageDocument("<p>a   b\n c</p>", SCOPE).stats).toEqual({ words: 3, characters: 5, paragraphs: 1 });
  });

  it("lists document images as rows and never plain links", () => {
    const doc = parsePageDocument(
      '<p>See <a href="https://example.com/doc.pdf">doc</a></p><img src="/api/assets/a.png" alt="Sunset">',
      SCOPE
    );
    expect(doc.assets).toHaveLength(1);
    expect(doc.assets[0]).toMatchObject({ source: "/api/assets/a.png", name: "Sunset", kind: "image" });
  });

  it("reads stored editor picture nodes too", () => {
    const doc = parsePageDocument('<image-component src="asset-9" alt="Chart"></image-component>', SCOPE);
    expect(doc.assets[0]?.source).toBe("asset-9");
    expect(doc.assets[0]?.name).toBe("Chart");
  });

  it("falls back through title, then file name for the row name", () => {
    expect(parsePageDocument('<img src="https://x.test/set/hill.png" title="A hill">', SCOPE).assets[0]?.name).toBe(
      "A hill"
    );
    expect(parsePageDocument('<img src="https://x.test/set/hill.png">', SCOPE).assets[0]?.name).toBe("hill.png");
    expect(parsePageDocument('<img src="https://x.test/" alt="  ">', SCOPE).assets[0]?.name).toBe("Image");
  });

  it("lists other attachment kinds without a thumbnail for the extension renderer", () => {
    const doc = parsePageDocument('<attachment-component src="asset-7" title="Brief"></attachment-component>', SCOPE);
    expect(doc.assets[0]).toMatchObject({
      source: "asset-7",
      name: "Brief",
      kind: "attachment",
      thumbnailHref: null,
    });
    // A nameless attachment still gets a readable row name.
    expect(
      parsePageDocument('<attachment-component src="asset-8"></attachment-component>', SCOPE).assets[0]?.name
    ).toBe("asset-8");
  });

  it("skips pictures with no source", () => {
    expect(parsePageDocument("<img>", SCOPE).assets).toEqual([]);
  });

  it("returns an empty document where no DOM parser exists", () => {
    const scoped = globalThis as { DOMParser?: unknown };
    const saved = scoped.DOMParser;
    scoped.DOMParser = undefined;
    try {
      expect(parsePageDocument("<h1>Top</h1>", SCOPE)).toEqual(EMPTY_PAGE_DOCUMENT);
    } finally {
      scoped.DOMParser = saved;
    }
  });
});

describe("asset URLs", () => {
  it("builds project view and download paths for a bare asset id", () => {
    expect(pageAssetViewHref("asset-9", SCOPE)).toBe("/api/assets/v2/workspaces/acme/projects/project-1/asset-9/");
    expect(pageAssetDownloadHref("asset-9", SCOPE)).toBe(
      "/api/assets/v2/workspaces/acme/projects/project-1/download/asset-9/"
    );
  });

  it("builds workspace paths when the page has no project", () => {
    const scope = { workspaceSlug: "acme", projectId: null };
    expect(pageAssetViewHref("asset-9", scope)).toBe("/api/assets/v2/workspaces/acme/asset-9/");
    expect(pageAssetDownloadHref("asset-9", scope)).toBe("/api/assets/v2/workspaces/acme/download/asset-9/");
  });

  it("keeps absolute, data, blob and empty sources as they are", () => {
    for (const src of ["https://cdn.test/a.png", "data:image/png;base64,AA", "blob:abc"]) {
      expect(pageAssetViewHref(src, SCOPE)).toBe(src);
      expect(pageAssetDownloadHref(src, SCOPE)).toBe(src);
    }
    expect(pageAssetViewHref("   ", SCOPE)).toBe("");
    expect(pageAssetDownloadHref("", SCOPE)).toBe("");
  });

  it("switches a served picture to the download leg and leaves other paths alone", () => {
    const served = "/api/assets/v2/workspaces/acme/projects/project-1/asset-9/";
    expect(pageAssetDownloadHref(served, SCOPE)).toBe(
      "/api/assets/v2/workspaces/acme/projects/project-1/download/asset-9/"
    );
    expect(pageAssetDownloadHref("/images/hill.png", SCOPE)).toBe("/images/hill.png");
    expect(pageAssetDownloadHref("/api/assets/v2/workspaces/other/projects/project-1/asset-9/", SCOPE)).toBe(
      "/api/assets/v2/workspaces/other/projects/project-1/asset-9/"
    );
  });

  it("escapes asset ids before they join a path", () => {
    expect(pageAssetViewHref("a/b c", SCOPE)).toContain("a%2Fb%20c");
  });

  it("names a row from the file name, decoding escapes", () => {
    expect(assetFileName("https://x.test/a/My%20Hill.png?w=2#top")).toBe("My Hill.png");
    expect(assetFileName("https://x.test/a/")).toBe("a");
    expect(assetFileName("https://x.test/")).toBe("");
    expect(assetFileName("https://x.test/a/%E0%A4%A")).toBe("%E0%A4%A");
  });
});

describe("read time", () => {
  it("estimates 200 words per minute", () => {
    expect(pageReadTimeSeconds(200)).toBe(60);
    expect(pageReadTimeSeconds(400)).toBe(120);
    expect(pageReadTimeSeconds(-5)).toBe(0);
  });

  it("reads seconds under a minute and minutes above", () => {
    expect(pageReadTimeLabel(0)).toBe("0s");
    expect(pageReadTimeLabel(100)).toBe("30s");
    // 199 words is 59.7s, which rounds up to the minute boundary.
    expect(pageReadTimeLabel(199)).toBe("1m");
    expect(pageReadTimeLabel(200)).toBe("1m");
    expect(pageReadTimeLabel(201)).toBe("1m");
    // 1201 words is 360.3s; whole seconds first, then whole minutes.
    expect(pageReadTimeLabel(1201)).toBe("6m");
  });
});

describe("attribution", () => {
  const NOW = Date.parse("2026-10-25T12:00:00Z");
  const members = [{ id: "u-ada", name: "Ada Byron", avatarUrl: "/api/assets/ada/" }];

  it("names and links both rows with relative and calendar times", () => {
    const rows = pageActorRows({
      workspaceSlug: "acme",
      updatedBy: "u-ada",
      updatedAt: "2026-10-25T09:00:00Z",
      ownedBy: "u-ada",
      createdAt: "2026-09-01T12:00:00Z",
      members,
      nowMs: NOW,
    });
    expect(rows).toEqual([
      {
        role: "edited",
        userId: "u-ada",
        name: "Ada Byron",
        deactivated: false,
        avatarUrl: "/api/assets/ada/",
        timeText: "3h ago",
        href: "/acme/profile/u-ada/",
      },
      {
        role: "created",
        userId: "u-ada",
        name: "Ada Byron",
        deactivated: false,
        avatarUrl: "/api/assets/ada/",
        timeText: "Sep 1, 2026",
        href: "/acme/profile/u-ada/",
      },
    ]);
  });

  it("falls back to the deactivated label while keeping the row and its link", () => {
    const rows = pageActorRows({
      workspaceSlug: "acme",
      updatedBy: "u-gone",
      updatedAt: "2026-10-25T11:59:30Z",
      ownedBy: "u-ada",
      createdAt: "2026-09-01T12:00:00Z",
      members,
      nowMs: NOW,
    });
    expect(rows[0]).toMatchObject({
      name: PAGE_DEACTIVATED_USER,
      deactivated: true,
      avatarUrl: null,
      href: "/acme/profile/u-gone/",
      timeText: "30s ago",
    });
  });

  it("renders both rows with no directory and no user ids", () => {
    const rows = pageActorRows({
      workspaceSlug: "acme",
      updatedBy: null,
      updatedAt: "bogus",
      ownedBy: null,
      createdAt: "bogus",
      members: undefined,
    });
    expect(rows.map((row) => [row.name, row.href, row.deactivated])).toEqual([
      [PAGE_DEACTIVATED_USER, null, true],
      [PAGE_DEACTIVATED_USER, null, true],
    ]);
    expect(rows[1]?.timeText).toBe("bogus");
  });

  it("projects the directory read down to the fields the rows use", () => {
    expect(pageActorMembers(undefined)).toBeUndefined();
    expect(
      pageActorMembers([
        {
          id: "m-1",
          role: 15,
          member: { id: "u-1", display_name: "Ada", avatar_url: "/api/assets/ada/" },
        } as never,
      ])
    ).toEqual([{ id: "u-1", name: "Ada", avatarUrl: "/api/assets/ada/" }]);
  });

  it("names a stored user, falling back for a departure or no id", () => {
    expect(pageMemberName(members, "u-ada")).toBe("Ada Byron");
    expect(pageMemberName(members, "u-gone")).toBe(PAGE_DEACTIVATED_USER);
    expect(pageMemberName(undefined, "u-ada")).toBe(PAGE_DEACTIVATED_USER);
    expect(pageMemberName(members, null)).toBeNull();
  });

  it("builds the member-profile address", () => {
    expect(pageMemberProfileHref("acme", "u-1")).toBe("/acme/profile/u-1/");
  });

  it("ladders the relative age", () => {
    expect(shortRelativeAge("bogus", NOW)).toBe("0s");
    expect(shortRelativeAge("2026-10-25T11:59:15Z", NOW)).toBe("45s");
    expect(shortRelativeAge("2026-10-25T11:45:00Z", NOW)).toBe("15m");
    expect(shortRelativeAge("2026-10-25T09:00:00Z", NOW)).toBe("3h");
    expect(shortRelativeAge("2026-10-22T12:00:00Z", NOW)).toBe("3d");
    expect(shortRelativeAge("2026-08-25T12:00:00Z", NOW)).toBe("2mo");
    expect(shortRelativeAge("2024-10-25T12:00:00Z", NOW)).toBe("2y");
    expect(shortRelativeAge("2026-10-26T12:00:00Z", NOW)).toBe("0s");
  });

  it("keeps an unreadable creation date verbatim and formats a readable one", () => {
    expect(pageCalendarDate("nope")).toBe("nope");
    expect(pageCalendarDate("2026-03-04T12:00:00Z")).toMatch(/2026/);
  });
});

describe("preview bars", () => {
  it("draws one bar per heading with its level's width", () => {
    expect(pagePreviewBars([])).toEqual([]);
    expect(
      pagePreviewBars([
        { level: 1, text: "A", levelIndex: 1 },
        { level: 3, text: "B", levelIndex: 1 },
      ])
    ).toEqual([
      { level: 1, widthPx: PAGE_PREVIEW_BAR_WIDTH_PX[1] },
      { level: 3, widthPx: PAGE_PREVIEW_BAR_WIDTH_PX[3] },
    ]);
  });
});
