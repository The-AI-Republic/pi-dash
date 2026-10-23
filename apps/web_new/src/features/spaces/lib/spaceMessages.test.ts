// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { SPACE_MESSAGE_KEYS, spaceMessage, type SpaceMessageKey } from "./spaceMessages.js";

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
  });
});
