// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page creation rules (parity/specs/pages.md §3.6, §11.1; PAGE-007/008):
// the 255 cap on a trimmed title, the tab's inherited visibility and the
// mark the picker's draft narrows to.

import { PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  PAGE_QUICK_CREATE_NAME,
  PAGE_TITLE_MAX_LENGTH,
  PAGE_TITLE_TOO_LONG_MESSAGE,
  pageCreateInput,
  pageDraftSubmittable,
  pageMarkFromDraft,
  pageTitleLength,
  pageTitleMissing,
  pageTitleTooLong,
  quickCreateAccess,
} from "./pageCreate.js";

describe("page title rules", () => {
  it("counts the trimmed title so surrounding blank space never trips the cap", () => {
    expect(PAGE_TITLE_MAX_LENGTH).toBe(255);
    expect(pageTitleLength(`  ${"a".repeat(255)}  `)).toBe(255);
    expect(pageTitleTooLong("a".repeat(255))).toBe(false);
    expect(pageTitleTooLong("a".repeat(256))).toBe(true);
    expect(PAGE_TITLE_TOO_LONG_MESSAGE).toContain("255");
  });

  it("treats whitespace-only titles as missing", () => {
    expect(pageTitleMissing("   ")).toBe(true);
    expect(pageTitleMissing("")).toBe(true);
    expect(pageTitleMissing("Title")).toBe(false);
  });

  it("submits only when the title is neither missing nor over the cap", () => {
    expect(pageDraftSubmittable("")).toBe(false);
    expect(pageDraftSubmittable("a".repeat(256))).toBe(false);
    expect(pageDraftSubmittable("A page")).toBe(true);
  });
});

describe("quick create visibility", () => {
  it("creates restricted pages from the private tab and open ones elsewhere", () => {
    expect(quickCreateAccess("private")).toBe(PAGE_ACCESS_PRIVATE);
    expect(quickCreateAccess("public")).toBe(PAGE_ACCESS_PUBLIC);
    expect(quickCreateAccess("archived")).toBe(PAGE_ACCESS_PUBLIC);
    expect(PAGE_QUICK_CREATE_NAME).toBe("Untitled");
  });
});

describe("mark narrowing", () => {
  it("keeps an emoji draft", () => {
    expect(pageMarkFromDraft({ in_use: "emoji", emoji: { value: "1f600" } })).toEqual({
      in_use: "emoji",
      emoji: { value: "1f600" },
    });
  });

  it("drops an emoji draft with no value or a non-record emoji", () => {
    expect(pageMarkFromDraft({ in_use: "emoji", emoji: { value: "" } })).toBeUndefined();
    expect(pageMarkFromDraft({ in_use: "emoji", emoji: "1f600" })).toBeUndefined();
    expect(pageMarkFromDraft({ in_use: "emoji" })).toBeUndefined();
  });

  it("keeps an icon draft with its colour, and without one when unset", () => {
    expect(pageMarkFromDraft({ in_use: "icon", icon: { name: "rocket", color: "#ff0000" } })).toEqual({
      in_use: "icon",
      icon: { name: "rocket", color: "#ff0000" },
    });
    expect(pageMarkFromDraft({ in_use: "icon", icon: { name: "rocket", color: "" } })).toEqual({
      in_use: "icon",
      icon: { name: "rocket" },
    });
    expect(pageMarkFromDraft({ in_use: "icon", icon: { name: "rocket" } })).toEqual({
      in_use: "icon",
      icon: { name: "rocket" },
    });
  });

  it("drops an icon draft that names nothing", () => {
    expect(pageMarkFromDraft({ in_use: "icon", icon: { name: "" } })).toBeUndefined();
    expect(pageMarkFromDraft({ in_use: "icon", icon: null })).toBeUndefined();
    expect(pageMarkFromDraft({ in_use: "icon" })).toBeUndefined();
  });

  it("drops a cleared, uploaded or unknown draft", () => {
    expect(pageMarkFromDraft({})).toBeUndefined();
    expect(pageMarkFromDraft({ in_use: "upload", url: "https://x.test/a.png" })).toBeUndefined();
    expect(pageMarkFromDraft({ in_use: "other" })).toBeUndefined();
  });
});

describe("create body", () => {
  it("trims the title and always carries the visibility", () => {
    expect(pageCreateInput("  Launch notes  ", PAGE_ACCESS_PRIVATE)).toEqual({
      name: "Launch notes",
      access: PAGE_ACCESS_PRIVATE,
    });
  });

  it("adds the mark only when the draft named one", () => {
    const mark = { in_use: "icon", icon: { name: "rocket", color: "#ff0000" } } as const;
    expect(pageCreateInput("Launch notes", PAGE_ACCESS_PUBLIC, mark)).toEqual({
      name: "Launch notes",
      access: PAGE_ACCESS_PUBLIC,
      logo_props: mark,
    });
  });
});
