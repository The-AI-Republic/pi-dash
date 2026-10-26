// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member directory behind rendered mention chips (PAGE-041).

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  isCurrentMentionUser,
  mentionDirectoryVersion,
  mentionNameFor,
  resetMentionDirectory,
  setMentionDirectory,
  subscribeMentionDirectory,
} from "./mentionDirectory.js";

afterEach(() => {
  resetMentionDirectory();
});

describe("mention directory", () => {
  it("resolves a stored name to the member's current display name", () => {
    setMentionDirectory([{ id: "u-1", name: "Ada Lovelace" }], "u-1");
    expect(mentionNameFor("u-1", "Ada")).toBe("Ada Lovelace");
  });

  it("falls back to the stored name for a departed member", () => {
    setMentionDirectory([], null);
    expect(mentionNameFor("u-1", " Ada ")).toBe("Ada");
    expect(mentionNameFor(null, undefined)).toBe("");
  });

  it("falls back when the directory entry has a blank name", () => {
    setMentionDirectory([{ id: "u-1", name: "   " }], null);
    expect(mentionNameFor("u-1", "Stored")).toBe("Stored");
  });

  it("distinguishes only the signed-in member", () => {
    setMentionDirectory([{ id: "u-1", name: "Ada" }], "u-2");
    expect(isCurrentMentionUser("u-1")).toBe(false);
    expect(isCurrentMentionUser("u-2")).toBe(true);
    expect(isCurrentMentionUser(null)).toBe(false);
    expect(isCurrentMentionUser(undefined)).toBe(false);
  });

  it("notifies subscribers and exposes a new version on each publication", () => {
    const listener = vi.fn();
    const before = mentionDirectoryVersion();
    const unsubscribe = subscribeMentionDirectory(listener);
    setMentionDirectory([{ id: "u-1", name: "Ada" }], null);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(mentionDirectoryVersion()).toBeGreaterThan(before);
    unsubscribe();
    setMentionDirectory([], null);
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
