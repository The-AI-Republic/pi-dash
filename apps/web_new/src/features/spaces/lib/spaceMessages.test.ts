// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it } from "vitest";

import { LANGUAGE_STORAGE_KEY, registerLocale, resetLocalesForTests } from "../../../core/i18n/locales.js";

import {
  registerSpaceMessages,
  resetSpaceMessagesForTests,
  SPACE_MESSAGE_KEYS,
  spaceMessage,
  type SpaceMessageKey,
} from "./spaceMessages.js";

afterEach(() => {
  globalThis.localStorage?.clear();
  resetSpaceMessagesForTests();
  resetLocalesForTests();
});

describe("spaceMessage", () => {
  it("carries non-empty English text for every stable key", () => {
    expect(SPACE_MESSAGE_KEYS.length).toBeGreaterThan(0);
    for (const key of SPACE_MESSAGE_KEYS) {
      expect(spaceMessage(key).trim()).not.toBe("");
    }
  });

  it("keeps the oracle-pinned notice wordings exact", () => {
    expect(spaceMessage("notice.copySuccess.title")).toBe("Link copied!");
    expect(spaceMessage("notice.copySuccess.body")).toBe("Work item link copied to clipboard.");
    expect(spaceMessage("notice.commentFailure.title")).toBe("Error!");
    expect(spaceMessage("notice.commentFailure.body")).toBe("Comment could not be posted. Please try again.");
  });

  it("keeps every key unique", () => {
    expect(new Set<string>(SPACE_MESSAGE_KEYS).size).toBe(SPACE_MESSAGE_KEYS.length);
  });

  it("covers the whole key union", () => {
    const keys: readonly SpaceMessageKey[] = SPACE_MESSAGE_KEYS;
    expect(keys).toContain("theme.switchToDark");
    expect(keys).toContain("account.fallbackName");
    expect(keys).toContain("badge.product");
    expect(keys).toContain("progress.loading");
    expect(keys).toContain("loading.board");
    expect(keys).toContain("board.issuesFallback");
    expect(keys).toContain("failure.anchor.heading");
    expect(keys).toContain("failure.board.heading");
  });

  it("reads the stored subtree tag with English as the fallback", () => {
    expect(spaceMessage("loading.board")).toBe("Loading board");
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    expect(spaceMessage("loading.board")).toBe("Loading board");
  });

  it("prefers the registered pack entry and falls back key by key", () => {
    registerLocale({ tag: "fr", label: "Francais" });
    globalThis.localStorage.setItem(LANGUAGE_STORAGE_KEY, "fr");
    registerSpaceMessages("fr", { "loading.board": "Chargement du tableau" });
    expect(spaceMessage("loading.board")).toBe("Chargement du tableau");
    expect(spaceMessage("loading.legacy")).toBe("Resolving board");
    expect(spaceMessage("loading.board", "de")).toBe("Loading board");
  });

  it("merges packs that arrive in pieces", () => {
    registerSpaceMessages("fr", { "loading.board": "Chargement du tableau" });
    registerSpaceMessages("fr", { "loading.legacy": "Resolution du tableau" });
    expect(spaceMessage("loading.board", "fr")).toBe("Chargement du tableau");
    expect(spaceMessage("loading.legacy", "fr")).toBe("Resolution du tableau");
  });
});
