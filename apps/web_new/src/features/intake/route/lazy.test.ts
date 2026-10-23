// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake chunk boundary (NEWFRONT-869): the route renders the screen and
// the create dialog the chunk resolved, and refuses to hand either out
// before `beforeLoad` awaited it, so a direct render is a routing bug
// rather than a blank page.
import { describe, expect, it, vi } from "vitest";

describe("intake route chunk", () => {
  it("refuses to resolve the chunk before the route awaited it", async () => {
    vi.resetModules();
    const { resolvedIntakeRouteChunk } = await import("./lazy.js");
    expect(() => resolvedIntakeRouteChunk()).toThrow(/await loadIntakeRouteChunk/);
  });

  it("resolves the screen, the create dialog and the preload once and reuses the request", async () => {
    vi.resetModules();
    const { loadIntakeRouteChunk, resolvedIntakeRouteChunk } = await import("./lazy.js");
    const first = loadIntakeRouteChunk();
    expect(loadIntakeRouteChunk()).toBe(first);
    const chunk = await first;
    expect(typeof chunk.IntakeScreen).toBe("function");
    expect(typeof chunk.IntakeCreateDialog).toBe("function");
    expect(typeof chunk.preloadIntakeRoute).toBe("function");
    expect(resolvedIntakeRouteChunk()).toBe(chunk);
  });
});
