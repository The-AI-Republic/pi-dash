// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { assistantLandingHref, assistantThreadHref, readPendingMessage } from "./navigation.js";

describe("assistant addresses", () => {
  it("keeps the landing and conversation deep-link contract", () => {
    expect(assistantLandingHref("acme")).toBe("/acme/assistant");
    expect(assistantThreadHref("acme", "thread-1")).toBe("/acme/assistant/thread-1");
  });
});

describe("readPendingMessage", () => {
  it("reads a trimmed handoff message", () => {
    expect(readPendingMessage({ pendingMessage: "  hello  " })).toBe("hello");
  });

  it("ignores blank, missing and malformed state", () => {
    expect(readPendingMessage({ pendingMessage: "   " })).toBeNull();
    expect(readPendingMessage({ pendingMessage: 42 })).toBeNull();
    expect(readPendingMessage({})).toBeNull();
    expect(readPendingMessage(null)).toBeNull();
    expect(readPendingMessage("pendingMessage")).toBeNull();
  });
});
