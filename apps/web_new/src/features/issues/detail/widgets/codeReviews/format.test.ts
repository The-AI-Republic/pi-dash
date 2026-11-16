// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { CODE_REVIEW_URL_PLACEHOLDER, reviewBadge, reviewTitle } from "./format.js";

describe("reviewBadge", () => {
  it("puts merged first, from either the flag or the state", () => {
    expect(reviewBadge({ merged: true, draft: false, state: "closed" })).toBe("Merged");
    expect(reviewBadge({ merged: false, draft: false, state: "merged" })).toBe("Merged");
  });

  it("marks draft ahead of the open state", () => {
    expect(reviewBadge({ merged: false, draft: true, state: "open" })).toBe("Draft");
  });

  it("maps open and closed", () => {
    expect(reviewBadge({ merged: false, draft: false, state: "open" })).toBe("Open");
    expect(reviewBadge({ merged: false, draft: false, state: "closed" })).toBe("Closed");
  });
});

describe("reviewTitle", () => {
  it("uses the stored title and falls back to the URL", () => {
    expect(reviewTitle({ title: "Fix login", url: "https://github.com/a/b/pull/1" })).toBe("Fix login");
    expect(reviewTitle({ title: "   ", url: "https://github.com/a/b/pull/1" })).toBe("https://github.com/a/b/pull/1");
  });

  it("pins the paste placeholder", () => {
    expect(CODE_REVIEW_URL_PLACEHOLDER).toBe("Paste a pull request or merge request URL");
  });
});
