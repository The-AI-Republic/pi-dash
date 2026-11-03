// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The pages barrel exposes the list shell's public surface for later slices.

import { describe, expect, it } from "vitest";

import * as pages from "./index.js";
import { pageKeys } from "./api/keys.js";

describe("pages barrel", () => {
  it("exports the list model, components and query helpers", () => {
    expect(pages.DEFAULT_PAGE_SORT).toEqual({ key: "modified", direction: "descending" });
    expect(pages.parsePageListSearch({ type: "private" })).toEqual({ type: "private" });
    expect(typeof pages.PagesListScreen).toBe("function");
    expect(typeof pages.PageRow).toBe("function");
    expect(typeof pages.copyPageLink).toBe("function");
    expect(typeof pages.PagesQuickCreate).toBe("function");
    expect(typeof pages.PageCreateDialogEntry).toBe("function");
    expect(typeof pages.PageCreateDialog).toBe("function");
    expect(typeof pages.PageFavoriteStar).toBe("function");
    expect(typeof pages.PageMarkPicker).toBe("function");
    expect(typeof pages.usePageFavorite).toBe("function");
    expect(typeof pages.useCreatePage).toBe("function");
    expect(typeof pages.useQuickCreatePage).toBe("function");
    expect(typeof pages.useDuplicatePage).toBe("function");
    expect(typeof pages.usePageVisibilityWrite).toBe("function");
    expect(typeof pages.usePageLockWrite).toBe("function");
    expect(typeof pages.usePageArchiveWrite).toBe("function");
    expect(typeof pages.usePageDeleteWrite).toBe("function");
    expect(typeof pages.usePageMoveWrite).toBe("function");
    expect(typeof pages.PageDeleteDialog).toBe("function");
    expect(typeof pages.PageMoveDialog).toBe("function");
    expect(typeof pages.PageRowConnected).toBe("function");
    expect(typeof pages.PageDetailHeader).toBe("function");
    expect(typeof pages.PageDetailFavoriteStar).toBe("function");
    expect(typeof pages.PageDetailLockControl).toBe("function");
    expect(typeof pages.pageDetailMenuItems).toBe("function");
    expect(typeof pages.pageArchivedMarker).toBe("function");
    expect(pages.PAGE_HISTORY_ITEM).toBe("Version history");
    expect(pages.PAGE_EXPORT_ITEM).toBe("Export");
    expect(pages.PAGE_LOCK_LABEL).toBe("Lock");
    expect(pages.PAGE_LOCKED_LABEL).toBe("Locked");
    expect(pages.PAGE_UNLOCKED_LABEL).toBe("Unlocked");
    expect(pages.PAGE_DETAIL_FAVORITE_ADD_LABEL).toBe("Add to favorites");
    expect(pages.PAGE_DETAIL_FAVORITE_REMOVE_LABEL).toBe("Remove favorite");
    expect(pages.PAGES_REALTIME_TOPIC).toBe("pages");
    expect(pages.CREATE_PAGE_COMMAND_ID).toBe("pages.create-page");
    expect(pages.PAGE_TITLE_MAX_LENGTH).toBe(255);
    expect(pages.pageKeys).toBe(pageKeys);
  });

  it("builds the row menu in the pinned order and gates the duplicate entry", () => {
    const handlers = {
      openInNewTab: () => undefined,
      copyLink: () => undefined,
      duplicate: () => undefined,
      toggleVisibility: () => undefined,
      toggleLock: () => undefined,
      toggleArchive: () => undefined,
      requestDelete: () => undefined,
      requestMove: () => undefined,
    };
    const base = { manage: false, archived: false, locked: false, restricted: false, move: false };
    expect(pages.pageRowActions({ ...base, duplicate: true }, handlers).map((entry) => entry.label)).toEqual([
      "Open in new tab",
      "Copy link",
      "Make a copy",
    ]);
    expect(pages.pageRowActions({ ...base, duplicate: false }, handlers).map((entry) => entry.label)).toEqual([
      "Open in new tab",
      "Copy link",
    ]);
  });

  it("scopes collection and detail keys under one workspace pages root", () => {
    expect(pageKeys.all("acme")).toContain("pages");
    expect(pageKeys.list("acme", "project-1")).toEqual([...pageKeys.all("acme"), "list", "project-1"]);
    expect(pageKeys.detail("acme", "project-1", "page-1")).toEqual([
      ...pageKeys.all("acme"),
      "detail",
      "project-1",
      "page-1",
    ]);
  });

  it("exports the sync, offline, oversized and fallback surface (PAGE-061-064)", () => {
    expect(pages.PAGE_OFFLINE_LABEL).toBe("Offline");
    expect(typeof pages.useBrowserOnline).toBe("function");
    expect(typeof pages.usePageOfflineVisible).toBe("function");
    expect(typeof pages.PageOfflinePill).toBe("function");
    expect(pages.PAGE_CONTENT_TOO_LARGE).toBe("content_too_large");
    expect(typeof pages.reportPageRealtimeError).toBe("function");
    expect(typeof pages.subscribePageRealtimeErrors).toBe("function");
    expect(typeof pages.reportPageContentTooLarge).toBe("function");
    expect(typeof pages.usePageOversizedIncident).toBe("function");
    expect(typeof pages.PageOversizedStrip).toBe("function");
    expect(typeof pages.savePageDescriptionFallback).toBe("function");
    expect(typeof pages.usePageSaveFallback).toBe("function");
    expect(typeof pages.pageBinaryToBase64).toBe("function");
  });

  it("exports the collaboration session surface (PAGE-065-067)", () => {
    expect(typeof pages.usePageLiveSession).toBe("function");
    expect(typeof pages.usePageReadOnlySyncReport).toBe("function");
    expect(typeof pages.fetchPageLiveFallbackBinary).toBe("function");
    expect(typeof pages.PageLiveLoader).toBe("function");
  });
});
