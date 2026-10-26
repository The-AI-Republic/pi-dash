// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Block catalogue (parity/specs/pages.md §6.1, PAGE-037/038/040). The
// palette's exact order is what the oracle compares, so it is pinned
// here: every structural entry, the media group and both colour rows.

import { describe, expect, it } from "vitest";

import {
  embedHtml,
  emojiHtml,
  filterPickerCommands,
  filterSlashEntries,
  PAGE_EMBED_TAG,
  PAGE_EMOJI_DATA_TYPE,
  pageCatalogBlockKind,
  pageSlashLabels,
  slashQueryAt,
} from "./pageBlocks.js";

describe("page slash catalogue", () => {
  it("lists every structural entry and both colour rows in order", () => {
    expect(pageSlashLabels()).toEqual([
      "Text",
      "Heading 1",
      "Heading 2",
      "Heading 3",
      "Heading 4",
      "Heading 5",
      "Heading 6",
      "Numbered list",
      "Bulleted list",
      "To-do list",
      "Table",
      "Quote",
      "Code",
      "Image",
      "Callout",
      "Divider",
      "Emoji",
      "Default",
      "Gray",
      "Peach",
      "Pink",
      "Orange",
      "Green",
      "Light blue",
      "Dark blue",
      "Purple",
      "Default background",
      "Gray",
      "Peach",
      "Pink",
      "Orange",
      "Green",
      "Light blue",
      "Dark blue",
      "Purple",
    ]);
  });

  it("filters by a case-insensitive substring and returns the whole list for an empty query", () => {
    expect(filterSlashEntries("")).toHaveLength(35);
    expect(
      filterSlashEntries("HEAD")
        .map((entry) => entry.label)
        .slice(0, 2)
    ).toEqual(["Heading 1", "Heading 2"]);
    expect(filterSlashEntries("zzz")).toEqual([]);
  });

  it("reaches inline, underline, alignment, link and mention commands by query without changing the pinned catalogue", () => {
    expect(pageSlashLabels()).toHaveLength(35);
    expect(filterSlashEntries("inline").map((entry) => entry.label)).toEqual(["Inline code"]);
    expect(filterSlashEntries("under").map((entry) => entry.label)).toEqual(["Underline"]);
    expect(filterSlashEntries("align").map((entry) => entry.label)).toEqual([
      "Left align",
      "Center align",
      "Right align",
    ]);
    expect(filterSlashEntries("link").map((entry) => entry.label)).toEqual(["Link"]);
    expect(filterSlashEntries("mention").map((entry) => entry.label)).toEqual(["Mention"]);
    expect(filterSlashEntries("embed").map((entry) => entry.label)).toEqual(["Embed work item"]);
  });

  it("keeps the picker commands silent until the query has text", () => {
    expect(filterPickerCommands("   ")).toEqual([]);
    expect(filterPickerCommands("  EMB  ").map((entry) => entry.label)).toEqual(["Embed work item"]);
    expect(filterPickerCommands("ent").map((entry) => entry.label)).toEqual(["Entity reference"]);
  });

  it("appends cloud registry snippets after the community catalogue (PAGE-078)", () => {
    const snippet = { label: "Cloud callout", group: "Cloud", action: { type: "snippet", html: "<p>x</p>" } } as const;
    const unqueried = filterSlashEntries("", [snippet]);
    expect(unqueried).toHaveLength(36);
    expect(unqueried[35]).toEqual(snippet);
    expect(unqueried.slice(0, 35).map((entry) => entry.label)).toEqual(pageSlashLabels());
    expect(filterSlashEntries("callout", [snippet]).map((entry) => entry.label)).toEqual(["Callout", "Cloud callout"]);
    expect(filterSlashEntries("zzz", [snippet])).toEqual([]);
  });
});

describe("page body markup fragments", () => {
  it("escapes an embed's identifiers", () => {
    const html = embedHtml({
      id: "a&b",
      entity_identifier: 'q"x',
      project_identifier: "p<1",
      workspace_identifier: "ws",
      entity_name: "Issue > 1",
    });
    expect(html).toContain(`<${PAGE_EMBED_TAG}`);
    expect(html).toContain('id="a&amp;b"');
    expect(html).toContain('entity_identifier="q&quot;x"');
    expect(html).toContain('project_identifier="p&lt;1"');
    expect(html).toContain('entity_name="Issue &gt; 1"');
  });

  it("marks an inline emoji with its stored data type", () => {
    expect(emojiHtml("🎉")).toBe(`<span data-type="${PAGE_EMOJI_DATA_TYPE}">🎉</span>`);
  });
});

describe("slashQueryAt", () => {
  it("reads a slash at the start of a block or after whitespace", () => {
    expect(slashQueryAt("/hea")).toBe("hea");
    expect(slashQueryAt("words /quo")).toBe("quo");
  });

  it("ignores whitespace after the slash and mid-word slashes", () => {
    expect(slashQueryAt("/ ")).toBeNull();
    expect(slashQueryAt("a/b")).toBeNull();
    expect(slashQueryAt("no slash")).toBeNull();
  });
});

describe("pageCatalogBlockKind", () => {
  it("maps typography entries and returns null for node entries", () => {
    expect(pageCatalogBlockKind("text")).toBe("paragraph");
    expect(pageCatalogBlockKind("heading6")).toBe("heading6");
    expect(pageCatalogBlockKind("table")).toBeNull();
    expect(pageCatalogBlockKind("emoji")).toBeNull();
  });
});

describe("catalogue entry fallbacks", () => {
  it("maps every heading level and refuses a non-typography entry", () => {
    for (const level of [1, 2, 3, 4, 5, 6] as const) {
      expect(pageCatalogBlockKind(`heading${level}`)).toBe(`heading${level}`);
    }
    expect(pageCatalogBlockKind("callout")).toBeNull();
  });
});

describe("catalogue default arm", () => {
  it("returns null for every non-typography kind", () => {
    for (const kind of [
      "numbered",
      "bulleted",
      "todo",
      "table",
      "quote",
      "code",
      "image",
      "callout",
      "divider",
      "emoji",
    ] as const) {
      expect(pageCatalogBlockKind(kind)).toBeNull();
    }
  });
});
