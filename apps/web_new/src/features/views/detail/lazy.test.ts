// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

describe("saved-view detail chunk", () => {
  it("refuses to resolve the screen before the route awaited the chunk", async () => {
    vi.resetModules();
    const { resolvedSavedViewDetail } = await import("./lazy.js");
    expect(() => resolvedSavedViewDetail()).toThrow(/await loadSavedViewDetail/);
  });
});
