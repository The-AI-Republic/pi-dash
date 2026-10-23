// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake chunk boundary (NEWFRONT-869): the route renders the screen and
// the create dialog the chunk resolved, and refuses to hand either out
// before `beforeLoad` awaited it, so a direct render is a routing bug
// rather than a blank page.
import { describe, expect, it, vi } from "vitest";

// The chunk's real graph (screen, dialog, preload) takes seconds to
// transform and import; under a full-suite + coverage run on a loaded host
// that import outlasted the 30s per-test budget and failed the suite
// spuriously (NEWFRONT-875). The boundary contract under test — one
// request, reused while pending, nothing resolved before the route's
// await — does not need the real modules, so the chunk resolves fakes.
// The export-name wiring stays guarded by lazy.ts's `typeof import`
// types (a wrong name fails typecheck) and by the suites that import the
// real modules.
vi.mock("../components/IntakeScreen.js", () => ({
  IntakeScreen: () => null,
}));
vi.mock("../components/IntakeCreateDialog.js", () => ({
  IntakeCreateDialog: () => null,
}));
vi.mock("./preload.js", () => ({
  preloadIntakeRoute: () => Promise.resolve(),
}));

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
    const [screenModule, dialogModule, preloadModule] = await Promise.all([
      import("../components/IntakeScreen.js"),
      import("../components/IntakeCreateDialog.js"),
      import("./preload.js"),
    ]);
    expect(chunk.IntakeScreen).toBe(screenModule.IntakeScreen);
    expect(chunk.IntakeCreateDialog).toBe(dialogModule.IntakeCreateDialog);
    expect(chunk.preloadIntakeRoute).toBe(preloadModule.preloadIntakeRoute);
    expect(resolvedIntakeRouteChunk()).toBe(chunk);
  });
});
