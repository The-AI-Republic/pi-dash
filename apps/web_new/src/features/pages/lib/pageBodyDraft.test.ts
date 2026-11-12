// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page body draft seam (parity/specs/pages.md §8.1): the editor's newest
// local body per page, kept for the save slice (PAGE-064) to persist.

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearPageBodyDraft,
  getPageBodyDraft,
  resetPageBodyDrafts,
  setPageBodyDraft,
  subscribePageBodyDraft,
} from "./pageBodyDraft.js";

afterEach(() => {
  resetPageBodyDrafts();
});

describe("page body drafts", () => {
  it("holds the newest body per page and nothing for an unseen page", () => {
    expect(getPageBodyDraft("page-1")).toBe(null);
    setPageBodyDraft("page-1", "<p>first</p>");
    setPageBodyDraft("page-1", "<p>second</p>");
    setPageBodyDraft("page-2", "<p>other</p>");
    expect(getPageBodyDraft("page-1")).toBe("<p>second</p>");
    expect(getPageBodyDraft("page-2")).toBe("<p>other</p>");
  });

  it("forgets one page without touching the rest", () => {
    setPageBodyDraft("page-1", "<p>one</p>");
    setPageBodyDraft("page-2", "<p>two</p>");
    clearPageBodyDraft("page-1");
    expect(getPageBodyDraft("page-1")).toBe(null);
    expect(getPageBodyDraft("page-2")).toBe("<p>two</p>");
  });

  it("tolerates clearing a page with no draft", () => {
    expect(() => clearPageBodyDraft("missing")).not.toThrow();
  });

  it("notifies subscribers of each new body and stops after unsubscribe", () => {
    const listener = vi.fn();
    const unsubscribe = subscribePageBodyDraft(listener);
    setPageBodyDraft("page-1", "<p>typed</p>");
    expect(listener).toHaveBeenCalledWith("page-1", "<p>typed</p>");
    unsubscribe();
    setPageBodyDraft("page-1", "<p>later</p>");
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("clears drafts and listeners on reset", () => {
    const listener = vi.fn();
    subscribePageBodyDraft(listener);
    setPageBodyDraft("page-1", "<p>typed</p>");
    resetPageBodyDrafts();
    expect(getPageBodyDraft("page-1")).toBe(null);
    setPageBodyDraft("page-1", "<p>again</p>");
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
