// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { makeDirectoryEntry, makeModuleLink } from "../../testing.js";
import {
  MODULE_LINK_ADD_TITLE,
  MODULE_LINK_COPIED,
  MODULE_LINK_CREATED,
  MODULE_LINK_DELETED,
  MODULE_LINK_UPDATED,
  MODULE_LINK_UPDATE_TITLE,
  MODULE_LINKS_EMPTY,
  MODULE_LINKS_TITLE,
  defaultModuleLinkScheme,
  describeModuleLinkMeta,
  moduleLinkFavicon,
  moduleLinkTitle,
  moduleLinksNeedAuthors,
  resolveModuleLinkAuthor,
} from "./moduleLinksModel.js";

describe("module links model", () => {
  it("pins the section, dialog and notice wording", () => {
    expect(MODULE_LINKS_TITLE).toBe("Links");
    expect(MODULE_LINKS_EMPTY).toBe("No links added yet");
    expect(MODULE_LINK_ADD_TITLE).toBe("Add link");
    expect(MODULE_LINK_UPDATE_TITLE).toBe("Update link");
    expect(MODULE_LINK_CREATED).toBe("Module link created successfully.");
    expect(MODULE_LINK_UPDATED).toBe("Module link updated successfully.");
    expect(MODULE_LINK_DELETED).toBe("Module link deleted successfully.");
    expect(MODULE_LINK_COPIED).toBe("Copied to clipboard");
  });

  it("defaults a missing scheme to http and keeps explicit schemes", () => {
    expect(defaultModuleLinkScheme("example.com")).toBe("http://example.com");
    expect(defaultModuleLinkScheme("  docs.example.com/guide  ")).toBe("http://docs.example.com/guide");
    expect(defaultModuleLinkScheme("https://example.com")).toBe("https://example.com");
    expect(defaultModuleLinkScheme("http://example.com")).toBe("http://example.com");
    expect(defaultModuleLinkScheme("HTTP://EXAMPLE.COM")).toBe("HTTP://EXAMPLE.COM");
  });

  it("reads the row title from the title, falling back to the URL", () => {
    expect(moduleLinkTitle(makeModuleLink({ title: "Launch plan" }))).toBe("Launch plan");
    expect(moduleLinkTitle(makeModuleLink({ title: "  Padded  " }))).toBe("Padded");
    expect(moduleLinkTitle(makeModuleLink({ title: null, url: "https://plain.example.com" }))).toBe(
      "https://plain.example.com"
    );
    expect(moduleLinkTitle(makeModuleLink({ title: "   ", url: "https://blank.example.com" }))).toBe(
      "https://blank.example.com"
    );
  });

  it("derives the favicon from the URL origin", () => {
    expect(moduleLinkFavicon("https://docs.example.com/guide")).toBe("https://docs.example.com/favicon.ico");
    expect(moduleLinkFavicon("http://example.com")).toBe("http://example.com/favicon.ico");
    expect(moduleLinkFavicon("not a url")).toBeNull();
    expect(moduleLinkFavicon("")).toBeNull();
  });

  it("joins the creator against the directory, tolerating gaps", () => {
    const members = [makeDirectoryEntry()];
    const link = makeModuleLink();
    expect(resolveModuleLinkAuthor(members, link.created_by)).toBe("Ada Lovelace");
    expect(resolveModuleLinkAuthor(members, "00000000-0000-0000-0000-000000000000")).toBeNull();
    expect(resolveModuleLinkAuthor(undefined, link.created_by)).toBeNull();
    expect(resolveModuleLinkAuthor(members, null)).toBeNull();
    expect(resolveModuleLinkAuthor(members, undefined)).toBeNull();
  });

  it("sentences the meta from the available halves", () => {
    const nowMs = Date.parse("2026-10-02T10:00:00Z");
    expect(describeModuleLinkMeta("2026-10-02T09:59:00Z", "Ada Lovelace", nowMs)).toBe(
      "Added 1 minute ago by Ada Lovelace"
    );
    expect(describeModuleLinkMeta("2026-10-02T09:59:00Z", null, nowMs)).toBe("Added 1 minute ago");
    expect(describeModuleLinkMeta(undefined, "Ada Lovelace", nowMs)).toBe("Added by Ada Lovelace");
    expect(describeModuleLinkMeta(undefined, null, nowMs)).toBeNull();
  });

  it("asks for authors only when a row records a creator", () => {
    expect(moduleLinksNeedAuthors([])).toBe(false);
    expect(moduleLinksNeedAuthors([{ created_by: null }, {}])).toBe(false);
    expect(moduleLinksNeedAuthors([{ created_by: null }, makeModuleLink()])).toBe(true);
  });
});
